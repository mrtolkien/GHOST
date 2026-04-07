use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use url::Url;

use crate::constants::{
    YOUTUBE_MIN_SECTION_CHARS, YOUTUBE_MIN_TRANSCRIPT_CHARS, YOUTUBE_SECTION_MAX_CHARS,
};

use super::error::ConvertError;
use super::staging::{create_staging_dir, slug_from_source};

/// Metadata file written to the staging directory.
const METADATA_FILE: &str = "_metadata.json";

/// Multilingual Whisper model filename expected from the nix package.
const WHISPER_MODEL_FILENAME: &str = "ggml-base.bin";

/// Nix package name for the multilingual Whisper base model.
const WHISPER_MODEL_PACKAGE: &str = "nixpkgs#whisper-cpp-model-base";

/// Subtitle suffix markers that still represent the base language track.
const SUBTITLE_VARIANT_MARKERS: &[&str] = &["orig", "forced"];

/// Known YouTube hosts accepted by the converter.
const YOUTUBE_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
    "www.youtu.be",
];

#[derive(Debug)]
#[must_use]
pub struct YoutubeConvertResult {
    pub staging_dir: PathBuf,
    pub metadata: YoutubeMetadata,
    pub section_count: usize,
    pub chapter_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YoutubeMetadata {
    pub source_url: String,
    pub video_id: String,
    pub title: Option<String>,
    pub channel: Option<String>,
    pub published_at: Option<String>,
    pub duration_seconds: Option<u64>,
    pub language: Option<String>,
    pub transcript_source: TranscriptSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YoutubeStagingMetadata {
    #[serde(flatten)]
    pub metadata: YoutubeMetadata,
    pub section_count: usize,
    pub chapter_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSource {
    Manual,
    Auto,
    Whisper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptCue {
    start_seconds: u64,
    text: String,
}

impl TranscriptCue {
    fn new(start_seconds: u64, text: impl Into<String>) -> Self {
        Self {
            start_seconds,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptSection {
    start_seconds: u64,
    title: String,
    text: String,
}

#[derive(Debug, Clone)]
struct ChapterMarker {
    start_seconds: u64,
    title: Option<String>,
}

#[derive(Debug, Clone)]
struct TranscriptCandidate {
    cues: Vec<TranscriptCue>,
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YtDlpVideoMetadata {
    id: String,
    title: Option<String>,
    channel: Option<String>,
    upload_date: Option<String>,
    duration: Option<f64>,
    language: Option<String>,
    chapters: Option<Vec<YtDlpChapter>>,
}

#[derive(Debug, Deserialize)]
struct YtDlpChapter {
    title: Option<String>,
    start_time: Option<f64>,
}

#[tracing::instrument(
    name = "convert_youtube",
    skip_all,
    fields(url = url, staging_root = %staging_root.display())
)]
pub async fn convert_youtube(
    staging_root: &Path,
    url: &str,
) -> Result<YoutubeConvertResult, ConvertError> {
    let video_id = validate_video_url(url)?;
    let source_url = url.to_string();
    let raw_metadata = fetch_video_metadata(url).await?;
    let chapters = chapter_markers(raw_metadata.chapters.as_deref());

    let staging_slug = slug_from_source(url);
    let staging_dir = create_staging_dir(staging_root, &staging_slug)?;
    let scratch_dir = tempfile::tempdir()
        .map_err(|e| ConvertError::Conversion(format!("failed to create temp dir: {e}")))?;

    let mut attempts = Vec::new();
    let (cues, selected_language, transcript_source) = match try_subtitles(
        url,
        scratch_dir.path().join("manual").as_path(),
        raw_metadata.language.as_deref(),
        false,
    )
    .await
    {
        Ok(candidate) => (candidate.cues, candidate.language, TranscriptSource::Manual),
        Err(error) => {
            attempts.push(format!("manual captions unavailable: {error}"));
            match try_subtitles(
                url,
                scratch_dir.path().join("auto").as_path(),
                raw_metadata.language.as_deref(),
                true,
            )
            .await
            {
                Ok(candidate) => (candidate.cues, candidate.language, TranscriptSource::Auto),
                Err(error) => {
                    attempts.push(format!("auto captions unavailable: {error}"));
                    match try_whisper_fallback(url, scratch_dir.path()).await {
                        Ok(cues) => (
                            cues,
                            raw_metadata.language.clone(),
                            TranscriptSource::Whisper,
                        ),
                        Err(error) => {
                            attempts.push(format!("whisper fallback failed: {error}"));
                            return Err(ConvertError::Conversion(format!(
                                "failed to acquire transcript: {}",
                                attempts.join("; ")
                            )));
                        }
                    }
                }
            }
        }
    };

    let transcript_text = transcript_text(&cues);
    ensure_transcript_length(&transcript_text)?;

    let mut metadata = YoutubeMetadata {
        source_url,
        video_id,
        title: raw_metadata.title,
        channel: raw_metadata.channel,
        published_at: normalize_upload_date(raw_metadata.upload_date.as_deref()),
        duration_seconds: raw_metadata.duration.and_then(duration_seconds),
        language: selected_language.or(raw_metadata.language),
        transcript_source,
    };

    if metadata.video_id.is_empty() {
        metadata.video_id = raw_metadata.id;
    }

    let sections = build_sections(&cues, &chapters, &metadata.title);
    if sections.is_empty() {
        return Err(ConvertError::Conversion(
            "transcript produced no importable sections".into(),
        ));
    }

    write_sections(&staging_dir, &metadata, &sections)?;
    write_metadata(&staging_dir, &metadata, sections.len(), chapters.len())?;

    Ok(YoutubeConvertResult {
        staging_dir,
        metadata,
        section_count: sections.len(),
        chapter_count: chapters.len(),
    })
}

fn build_sections(
    cues: &[TranscriptCue],
    chapters: &[ChapterMarker],
    video_title: &Option<String>,
) -> Vec<TranscriptSection> {
    let chapter_starts: Vec<u64> = chapters
        .iter()
        .map(|chapter| chapter.start_seconds)
        .collect();
    let mut sections = split_sections(
        cues,
        &chapter_starts,
        YOUTUBE_SECTION_MAX_CHARS,
        YOUTUBE_MIN_SECTION_CHARS,
    );

    let chapter_indices: Vec<usize> = sections
        .iter()
        .map(|section| chapter_index_for(section.start_seconds, chapters))
        .collect();
    let mut chapter_totals = vec![0usize; chapters.len().max(1)];
    for chapter_index in &chapter_indices {
        chapter_totals[*chapter_index] += 1;
    }

    let mut chapter_part_counts = vec![0usize; chapters.len().max(1)];
    for (section, chapter_index) in sections.iter_mut().zip(chapter_indices) {
        chapter_part_counts[chapter_index] += 1;
        section.title = section_title(
            section.start_seconds,
            chapter_index,
            chapter_part_counts[chapter_index],
            chapter_totals[chapter_index],
            chapters,
            video_title.as_deref(),
        );
    }

    sections
}

fn split_sections(
    cues: &[TranscriptCue],
    chapter_starts: &[u64],
    max_chars: usize,
    min_chars: usize,
) -> Vec<TranscriptSection> {
    let normalized_cues: Vec<TranscriptCue> = cues
        .iter()
        .filter_map(|cue| {
            let text = cue.text.trim();
            (!text.is_empty()).then(|| TranscriptCue::new(cue.start_seconds, text))
        })
        .collect();
    if normalized_cues.is_empty() {
        return vec![];
    }

    let groups = group_cues_by_chapter(&normalized_cues, chapter_starts);
    let mut sections = Vec::new();
    for group in groups {
        sections.extend(split_group_by_length(&group, max_chars));
    }

    merge_small_sections(sections, max_chars, min_chars)
}

fn group_cues_by_chapter(
    cues: &[TranscriptCue],
    chapter_starts: &[u64],
) -> Vec<Vec<TranscriptCue>> {
    if cues.is_empty() {
        return vec![];
    }

    let mut starts: Vec<u64> = chapter_starts.to_vec();
    starts.sort_unstable();
    starts.dedup();
    if starts.is_empty() || starts[0] > cues[0].start_seconds {
        starts.insert(0, cues[0].start_seconds);
    }

    let mut groups: Vec<Vec<TranscriptCue>> = Vec::new();
    let mut current_group = Vec::new();
    let mut boundary_index = 1usize;

    for cue in cues {
        while boundary_index < starts.len() && cue.start_seconds >= starts[boundary_index] {
            if !current_group.is_empty() {
                groups.push(current_group);
                current_group = Vec::new();
            }
            boundary_index += 1;
        }
        current_group.push(cue.clone());
    }

    if !current_group.is_empty() {
        groups.push(current_group);
    }

    groups
}

fn split_group_by_length(cues: &[TranscriptCue], max_chars: usize) -> Vec<TranscriptSection> {
    let mut sections = Vec::new();
    let mut current = Vec::new();

    for cue in cues {
        let cue_parts = split_large_cue(cue, max_chars);
        for part in cue_parts {
            let projected_len = cues_text_len(&current)
                + separator_len(&current)
                + part.text.len()
                + usize::from(!current.is_empty()) * 2;
            if !current.is_empty() && projected_len > max_chars {
                sections.push(section_from_cues(&current));
                current = Vec::new();
            }
            current.push(part);
        }
    }

    if !current.is_empty() {
        sections.push(section_from_cues(&current));
    }

    sections
}

fn split_large_cue(cue: &TranscriptCue, max_chars: usize) -> Vec<TranscriptCue> {
    if cue.text.len() <= max_chars {
        return vec![cue.clone()];
    }

    chunk_text(&cue.text, max_chars)
        .into_iter()
        .map(|chunk| TranscriptCue::new(cue.start_seconds, chunk))
        .collect()
}

fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < text.len() {
        let mut end = (start + max_chars).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            break;
        }
        chunks.push(text[start..end].trim().to_string());
        start = end;
    }

    chunks.retain(|chunk| !chunk.is_empty());
    chunks
}

fn merge_small_sections(
    sections: Vec<TranscriptSection>,
    max_chars: usize,
    min_chars: usize,
) -> Vec<TranscriptSection> {
    let mut merged = Vec::new();
    let mut index = 0usize;

    while index < sections.len() {
        let mut current = sections[index].clone();

        while current.text.len() < min_chars
            && index + 1 < sections.len()
            && merged_text_len(&current, &sections[index + 1]) <= max_chars
        {
            index += 1;
            current = merge_section_pair(current, sections[index].clone());
        }

        if current.text.len() < min_chars
            && let Some(previous) = merged.pop()
        {
            if merged_text_len(&previous, &current) <= max_chars {
                merged.push(merge_section_pair(previous, current));
            } else {
                merged.push(previous);
                merged.push(current);
            }
        } else {
            merged.push(current);
        }

        index += 1;
    }

    merged
}

fn merge_section_pair(left: TranscriptSection, right: TranscriptSection) -> TranscriptSection {
    TranscriptSection {
        start_seconds: left.start_seconds,
        title: left.title,
        text: [left.text, right.text].join("\n\n"),
    }
}

fn section_from_cues(cues: &[TranscriptCue]) -> TranscriptSection {
    TranscriptSection {
        start_seconds: cues[0].start_seconds,
        title: String::new(),
        text: cues
            .iter()
            .map(|cue| cue.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

fn cues_text_len(cues: &[TranscriptCue]) -> usize {
    cues.iter().map(|cue| cue.text.len()).sum::<usize>()
}

fn separator_len<T>(items: &[T]) -> usize {
    items.len().saturating_sub(1) * 2
}

fn merged_text_len(left: &TranscriptSection, right: &TranscriptSection) -> usize {
    left.text.len() + right.text.len() + 2
}

fn chapter_markers(chapters: Option<&[YtDlpChapter]>) -> Vec<ChapterMarker> {
    let Some(chapters) = chapters else {
        return vec![];
    };

    let mut markers: Vec<ChapterMarker> = chapters
        .iter()
        .filter_map(|chapter| {
            let start_seconds = chapter.start_time.and_then(duration_seconds)?;
            Some(ChapterMarker {
                start_seconds,
                title: chapter
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty()),
            })
        })
        .collect();
    markers.sort_by_key(|chapter| chapter.start_seconds);
    markers.dedup_by_key(|chapter| chapter.start_seconds);
    markers
}

fn chapter_index_for(start_seconds: u64, chapters: &[ChapterMarker]) -> usize {
    chapters
        .iter()
        .rposition(|chapter| start_seconds >= chapter.start_seconds)
        .unwrap_or(0)
}

fn section_title(
    start_seconds: u64,
    chapter_index: usize,
    part_index: usize,
    part_total: usize,
    chapters: &[ChapterMarker],
    video_title: Option<&str>,
) -> String {
    let base = chapters
        .get(chapter_index)
        .and_then(|chapter| chapter.title.as_deref())
        .map(str::to_string)
        .or_else(|| {
            video_title.map(|title| format!("{title} @ {}", format_timestamp(start_seconds)))
        })
        .unwrap_or_else(|| format!("Transcript {}", format_timestamp(start_seconds)));

    if part_total > 1 {
        format!("{base} (Part {part_index})")
    } else {
        base
    }
}

fn timestamp_slug(seconds: u64) -> String {
    format!("{:02}{:02}", seconds / 60, seconds % 60)
}

fn format_timestamp(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn validate_video_url(url: &str) -> Result<String, ConvertError> {
    let parsed = Url::parse(url)
        .map_err(|e| ConvertError::Conversion(format!("invalid YouTube URL: {e}")))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| ConvertError::Conversion("YouTube URL has no host".into()))?;
    if !YOUTUBE_HOSTS
        .iter()
        .any(|candidate| host.eq_ignore_ascii_case(candidate))
    {
        return Err(ConvertError::Conversion(format!(
            "unsupported YouTube host: {host}"
        )));
    }

    if parsed.query_pairs().any(|(key, _)| key == "list") {
        return Err(ConvertError::Conversion(
            "playlist URLs are not supported for YouTube import".into(),
        ));
    }

    let path = parsed.path().trim_matches('/');
    if path.starts_with("channel/")
        || path.starts_with("user/")
        || path.starts_with("c/")
        || path.starts_with('@')
        || path == "playlist"
    {
        return Err(ConvertError::Conversion(
            "channel, user, and playlist URLs are not supported".into(),
        ));
    }

    extract_video_id(&parsed).ok_or_else(|| {
        ConvertError::Conversion("could not recover a YouTube video id from URL".into())
    })
}

fn extract_video_id(url: &Url) -> Option<String> {
    match url.host_str()? {
        "youtu.be" | "www.youtu.be" => url
            .path_segments()?
            .find(|segment| !segment.is_empty())
            .map(str::to_string),
        _ => {
            let path = url.path().trim_matches('/');
            if path == "watch" {
                url.query_pairs()
                    .find_map(|(key, value)| (key == "v" && !value.is_empty()).then_some(value))
                    .map(std::borrow::Cow::into_owned)
            } else if let Some(id) = path.strip_prefix("shorts/") {
                (!id.is_empty()).then(|| id.to_string())
            } else if let Some(id) = path.strip_prefix("embed/") {
                (!id.is_empty()).then(|| id.to_string())
            } else if let Some(id) = path.strip_prefix("live/") {
                (!id.is_empty()).then(|| id.to_string())
            } else {
                None
            }
        }
    }
}

fn ensure_transcript_length(text: &str) -> Result<(), ConvertError> {
    let trimmed = text.trim();
    if trimmed.len() < YOUTUBE_MIN_TRANSCRIPT_CHARS {
        return Err(ConvertError::Conversion(format!(
            "transcript too short: {} chars (minimum {})",
            trimmed.len(),
            YOUTUBE_MIN_TRANSCRIPT_CHARS
        )));
    }

    Ok(())
}

fn transcript_text(cues: &[TranscriptCue]) -> String {
    cues.iter()
        .map(|cue| cue.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn normalize_upload_date(upload_date: Option<&str>) -> Option<String> {
    let upload_date = upload_date?;
    if upload_date.len() == 8 && upload_date.chars().all(|ch| ch.is_ascii_digit()) {
        Some(format!(
            "{}-{}-{}",
            &upload_date[0..4],
            &upload_date[4..6],
            &upload_date[6..8]
        ))
    } else {
        Some(upload_date.to_string())
    }
}

fn duration_seconds(duration: f64) -> Option<u64> {
    (duration.is_finite() && duration >= 0.0 && duration <= u64::MAX as f64)
        .then(|| duration.round() as u64)
}

async fn fetch_video_metadata(url: &str) -> Result<YtDlpVideoMetadata, ConvertError> {
    let output = run_yt_dlp_json(url).await?;

    serde_json::from_str(&output)
        .map_err(|e| ConvertError::Conversion(format!("failed to parse yt-dlp metadata: {e}")))
}

async fn run_yt_dlp_json(url: &str) -> Result<String, ConvertError> {
    let mut command = Command::new("nix");
    command
        .args(["shell", "nixpkgs#yt-dlp", "--command", "yt-dlp"])
        .args(["--dump-single-json", "--no-warnings", "--no-playlist", url]);

    run_command(&mut command, "yt-dlp metadata fetch").await
}

async fn try_subtitles(
    url: &str,
    output_dir: &Path,
    preferred_language: Option<&str>,
    auto: bool,
) -> Result<TranscriptCandidate, ConvertError> {
    let files = run_yt_dlp_subtitles(url, output_dir, auto).await?;
    load_best_transcript(&files, preferred_language, YOUTUBE_MIN_TRANSCRIPT_CHARS)
}

async fn try_whisper_fallback(
    url: &str,
    scratch_dir: &Path,
) -> Result<Vec<TranscriptCue>, ConvertError> {
    let audio_path = run_yt_dlp_audio(url, &scratch_dir.join("audio")).await?;
    let transcript_path = run_whisper(&audio_path, &scratch_dir.join("whisper")).await?;
    let cues = parse_vtt_file(&transcript_path)?;
    ensure_transcript_length(&transcript_text(&cues))?;
    Ok(cues)
}

async fn run_yt_dlp_subtitles(
    url: &str,
    output_dir: &Path,
    auto: bool,
) -> Result<Vec<PathBuf>, ConvertError> {
    std::fs::create_dir_all(output_dir)?;
    let output_template = output_dir.join("%(id)s.%(ext)s");

    let mut command = Command::new("nix");
    command
        .args(["shell", "nixpkgs#yt-dlp", "--command", "yt-dlp"])
        .args([
            "--skip-download",
            "--sub-format",
            "vtt",
            "--sub-langs",
            "all",
        ])
        .arg("--output")
        .arg(output_template)
        .arg(url);

    if auto {
        command.arg("--write-auto-sub");
    } else {
        command.arg("--write-sub");
    }

    run_command(
        &mut command,
        if auto {
            "yt-dlp automatic caption download"
        } else {
            "yt-dlp manual caption download"
        },
    )
    .await?;

    let mut files = collect_files_with_extension(output_dir, "vtt")?;
    files.sort();
    if files.is_empty() {
        return Err(ConvertError::Conversion(
            "no subtitle files were produced".into(),
        ));
    }
    Ok(files)
}

async fn run_yt_dlp_audio(url: &str, output_dir: &Path) -> Result<PathBuf, ConvertError> {
    std::fs::create_dir_all(output_dir)?;
    let output_template = output_dir.join("%(id)s.%(ext)s");

    let mut command = Command::new("nix");
    command
        .args(["shell", "nixpkgs#yt-dlp", "--command", "yt-dlp"])
        .args([
            "-f",
            "bestaudio/best",
            "--extract-audio",
            "--audio-format",
            "wav",
        ])
        .arg("--output")
        .arg(output_template)
        .arg(url);

    run_command(&mut command, "yt-dlp audio download").await?;

    collect_files_with_extension(output_dir, "wav")?
        .into_iter()
        .next()
        .ok_or_else(|| ConvertError::Conversion("yt-dlp produced no audio file".into()))
}

async fn run_whisper(audio_path: &Path, output_dir: &Path) -> Result<PathBuf, ConvertError> {
    std::fs::create_dir_all(output_dir)?;
    let output_prefix = output_dir.join("transcript");
    let output_vtt = output_prefix.with_extension("vtt");

    let script = format!(
        "model=\"$(find /nix/store -path '*whisper-cpp-model-base*' -name '{}' | head -n 1)\"; \
         if [ -z \"$model\" ]; then echo 'no whisper.cpp model found in nix shell' >&2; exit 1; fi; \
         whisper-cli -m \"$model\" -f '{}' -ovtt -of '{}'",
        WHISPER_MODEL_FILENAME,
        audio_path.display(),
        output_prefix.display()
    );

    let mut command = Command::new("nix");
    command.args([
        "shell",
        "nixpkgs#whisper-cpp",
        WHISPER_MODEL_PACKAGE,
        "--command",
        "sh",
        "-lc",
        &script,
    ]);

    run_command(&mut command, "whisper transcription").await?;

    if !output_vtt.exists() {
        return Err(ConvertError::Conversion(format!(
            "whisper produced no VTT transcript at {}",
            output_vtt.display()
        )));
    }

    Ok(output_vtt)
}

async fn run_command(command: &mut Command, action: &str) -> Result<String, ConvertError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = command
        .output()
        .await
        .map_err(|e| ConvertError::Conversion(format!("failed to spawn {action}: {e}")))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(ConvertError::Conversion(format!(
        "{action} failed: {}",
        if detail.is_empty() {
            "command exited unsuccessfully".into()
        } else {
            detail
        }
    )))
}

fn collect_files_with_extension(dir: &Path, extension: &str) -> Result<Vec<PathBuf>, ConvertError> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension() == Some(OsStr::new(extension)) {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn load_best_transcript(
    paths: &[PathBuf],
    preferred_language: Option<&str>,
    min_chars: usize,
) -> Result<TranscriptCandidate, ConvertError> {
    let preferred: Vec<PathBuf> = paths
        .iter()
        .filter(|path| subtitle_matches_language(path, preferred_language))
        .cloned()
        .collect();

    if !preferred.is_empty() {
        let (preferred_best, preferred_error) = pick_longest_transcript(&preferred, min_chars);
        if let Some(candidate) = preferred_best {
            return Ok(candidate);
        }

        let (all_best, all_error) = pick_longest_transcript(paths, min_chars);
        if let Some(candidate) = all_best {
            return Ok(candidate);
        }

        if let Some(error) = preferred_error.or(all_error) {
            return Err(error);
        }
        return Err(ConvertError::Conversion(
            "subtitle files contained no transcript cues".into(),
        ));
    }

    let (best, error) = pick_longest_transcript(paths, min_chars);
    if let Some(candidate) = best {
        return Ok(candidate);
    }
    if let Some(error) = error {
        return Err(error);
    }
    Err(ConvertError::Conversion(
        "subtitle files contained no transcript cues".into(),
    ))
}

fn pick_longest_transcript(
    paths: &[PathBuf],
    min_chars: usize,
) -> (Option<TranscriptCandidate>, Option<ConvertError>) {
    let mut best: Option<TranscriptCandidate> = None;
    let mut best_len = 0usize;
    let mut first_error = None;

    for path in paths {
        let cues = match parse_vtt_file(path) {
            Ok(cues) => cues,
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                continue;
            }
        };
        let text_len = transcript_text(&cues).len();
        if text_len < min_chars {
            continue;
        }
        if text_len > best_len {
            best_len = text_len;
            best = Some(TranscriptCandidate {
                cues,
                language: subtitle_language_from_path(path),
            });
        }
    }

    (best, first_error)
}

fn subtitle_language_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let language = stem.rsplit_once('.')?.1;
    (!language.is_empty()).then(|| language.to_ascii_lowercase())
}

fn subtitle_matches_language(path: &Path, preferred_language: Option<&str>) -> bool {
    let Some(preferred_language) = preferred_language else {
        return false;
    };
    let preferred_language = preferred_language.to_ascii_lowercase();
    let normalized_preferred = preferred_language.replace('_', "-");
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let file_name = file_name.to_ascii_lowercase();

    if file_name.contains(&format!(".{preferred_language}."))
        || file_name.contains(&format!(".{normalized_preferred}."))
        || file_name.contains(&format!(".{preferred_language}-"))
        || file_name.contains(&format!(".{normalized_preferred}-"))
    {
        return true;
    }

    normalized_preferred.split('-').next().is_some_and(|base| {
        normalized_preferred.contains('-')
            && (file_name.contains(&format!(".{base}."))
                || SUBTITLE_VARIANT_MARKERS
                    .iter()
                    .any(|marker| file_name.contains(&format!(".{base}-{marker}."))))
    })
}

fn parse_vtt_file(path: &Path) -> Result<Vec<TranscriptCue>, ConvertError> {
    let content = std::fs::read_to_string(path)?;
    parse_vtt(&content)
}

fn parse_vtt(content: &str) -> Result<Vec<TranscriptCue>, ConvertError> {
    let normalized = content.replace("\r\n", "\n");
    let mut cues = Vec::new();

    for block in normalized.split("\n\n") {
        let mut lines = block.lines().map(str::trim).filter(|line| !line.is_empty());
        let Some(first) = lines.next() else {
            continue;
        };

        let timing_line = if first.contains("-->") {
            first
        } else {
            let Some(next) = lines.next() else {
                continue;
            };
            if !next.contains("-->") {
                continue;
            }
            next
        };

        let Some(start) = timing_line.split("-->").next() else {
            continue;
        };
        let start_seconds = parse_vtt_timestamp(start.trim())?;
        let text = lines
            .filter(|line| !line.starts_with("NOTE"))
            .collect::<Vec<_>>()
            .join(" ")
            .replace("&nbsp;", " ")
            .trim()
            .to_string();

        if !text.is_empty() {
            cues.push(TranscriptCue::new(start_seconds, text));
        }
    }

    Ok(cues)
}

fn parse_vtt_timestamp(timestamp: &str) -> Result<u64, ConvertError> {
    let raw = timestamp.replace(',', ".");
    let parts: Vec<&str> = raw.split(':').collect();
    let seconds = match parts.as_slice() {
        [hours, minutes, seconds] => {
            parse_u64(hours)? * 3_600 + parse_u64(minutes)? * 60 + parse_seconds_part(seconds)?
        }
        [minutes, seconds] => parse_u64(minutes)? * 60 + parse_seconds_part(seconds)?,
        _ => {
            return Err(ConvertError::Conversion(format!(
                "invalid VTT timestamp: {timestamp}"
            )));
        }
    };

    Ok(seconds)
}

fn parse_u64(value: &str) -> Result<u64, ConvertError> {
    value.parse::<u64>().map_err(|e| {
        ConvertError::Conversion(format!("invalid timestamp component '{value}': {e}"))
    })
}

fn parse_seconds_part(value: &str) -> Result<u64, ConvertError> {
    let whole = value.split('.').next().unwrap_or(value);
    parse_u64(whole)
}

fn write_sections(
    staging_dir: &Path,
    metadata: &YoutubeMetadata,
    sections: &[TranscriptSection],
) -> Result<(), ConvertError> {
    for (index, section) in sections.iter().enumerate() {
        let filename = section_filename(index + 1, section);
        let content = format!(
            "# {}\n\nVideo: {}\nURL: {}\nStart: {}\n\n{}\n",
            section.title,
            metadata.title.as_deref().unwrap_or("Unknown video"),
            metadata.source_url,
            format_timestamp(section.start_seconds),
            section.text.trim(),
        );
        std::fs::write(staging_dir.join(filename), content)?;
    }

    Ok(())
}

fn section_filename(index: usize, section: &TranscriptSection) -> String {
    let mut slug = slugify_title(&section.title);
    if slug.is_empty() {
        slug = "transcript".into();
    }
    format!(
        "{index:02}-{}-{slug}.md",
        timestamp_slug(section.start_seconds)
    )
}

fn slugify_title(title: &str) -> String {
    let raw: String = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    raw.split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn write_metadata(
    staging_dir: &Path,
    metadata: &YoutubeMetadata,
    section_count: usize,
    chapter_count: usize,
) -> Result<(), ConvertError> {
    let staging_metadata = YoutubeStagingMetadata {
        metadata: metadata.clone(),
        section_count,
        chapter_count,
    };
    let json = serde_json::to_string_pretty(&staging_metadata)
        .map_err(|e| ConvertError::Conversion(format!("failed to serialize metadata: {e}")))?;
    std::fs::write(staging_dir.join(METADATA_FILE), json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn cue(start_seconds: u64, len: usize) -> TranscriptCue {
        TranscriptCue::new(start_seconds, "x".repeat(len))
    }

    #[test]
    fn split_sections_enforces_max_chars_without_chapters() {
        let cues = vec![cue(0, 25_000), cue(60, 25_000)];

        let sections = split_sections(
            &cues,
            &[],
            crate::constants::YOUTUBE_SECTION_MAX_CHARS,
            crate::constants::YOUTUBE_MIN_SECTION_CHARS,
        );

        assert_eq!(sections.len(), 2);
        assert!(
            sections
                .iter()
                .all(|section| section.text.len() <= crate::constants::YOUTUBE_SECTION_MAX_CHARS)
        );
    }

    #[test]
    fn split_sections_splits_oversized_chapter() {
        let cues = vec![cue(0, 20_000), cue(60, 20_000), cue(120, 20_000)];

        let sections = split_sections(
            &cues,
            &[0],
            crate::constants::YOUTUBE_SECTION_MAX_CHARS,
            crate::constants::YOUTUBE_MIN_SECTION_CHARS,
        );

        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].start_seconds, 0);
        assert_eq!(sections[1].start_seconds, 60);
        assert_eq!(sections[2].start_seconds, 120);
    }

    #[test]
    fn split_group_by_length_accounts_for_separator_overhead() {
        let cues = vec![cue(0, 5), cue(60, 4)];

        let sections = split_group_by_length(&cues, 10);

        assert_eq!(sections.len(), 2);
        assert!(sections.iter().all(|section| section.text.len() <= 10));
    }

    #[test]
    fn split_sections_merges_tiny_adjacent_chapters() {
        let cues = vec![cue(0, 900), cue(60, 900), cue(120, 4_000)];

        let sections = split_sections(
            &cues,
            &[0, 60, 120],
            crate::constants::YOUTUBE_SECTION_MAX_CHARS,
            crate::constants::YOUTUBE_MIN_SECTION_CHARS,
        );

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].start_seconds, 0);
        assert!(sections[0].text.len() >= crate::constants::YOUTUBE_MIN_SECTION_CHARS);
    }

    #[test]
    fn timestamp_slug_is_stable_for_section_filenames() {
        assert_eq!(timestamp_slug(0), "0000");
        assert_eq!(timestamp_slug(8 * 60 + 40), "0840");
    }

    #[test]
    fn validate_video_url_rejects_playlist_urls() {
        let error = validate_video_url("https://www.youtube.com/watch?v=test123&list=playlist456")
            .expect_err("playlist URL should be rejected");

        assert!(error.to_string().contains("playlist"));
    }

    #[test]
    fn transcript_shorter_than_minimum_is_rejected() {
        let text = "x".repeat(crate::constants::YOUTUBE_MIN_TRANSCRIPT_CHARS - 1);
        let error = ensure_transcript_length(&text).expect_err("short transcript should fail");

        assert!(error.to_string().contains("too short"));
    }

    #[test]
    fn metadata_json_serialization_includes_transcript_source() {
        let metadata = YoutubeMetadata {
            source_url: "https://www.youtube.com/watch?v=test123".into(),
            video_id: "test123".into(),
            title: Some("Test Video".into()),
            channel: Some("Example Channel".into()),
            published_at: Some("2024-01-02".into()),
            duration_seconds: Some(123),
            language: Some("en".into()),
            transcript_source: TranscriptSource::Auto,
        };

        let json = serde_json::to_string_pretty(&metadata).expect("serialize metadata");

        assert!(json.contains("\"transcript_source\": \"auto\""));
    }

    #[test]
    fn subtitle_language_match_prefers_requested_track() {
        let english = Path::new("/tmp/test123.en.vtt");
        let japanese = Path::new("/tmp/test123.ja.vtt");
        let regional = Path::new("/tmp/test123.en-US.vtt");

        assert!(subtitle_matches_language(english, Some("en")));
        assert!(subtitle_matches_language(regional, Some("en")));
        assert!(subtitle_matches_language(japanese, Some("ja")));
        assert!(!subtitle_matches_language(japanese, Some("en")));
        assert!(!subtitle_matches_language(english, None));
    }

    #[test]
    fn whisper_model_package_is_multilingual() {
        assert_eq!(WHISPER_MODEL_PACKAGE, "nixpkgs#whisper-cpp-model-base");
        assert_eq!(WHISPER_MODEL_FILENAME, "ggml-base.bin");
    }

    #[test]
    fn load_best_transcript_prefers_requested_language_track() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let english = tempdir.path().join("video.en.vtt");
        let japanese = tempdir.path().join("video.ja.vtt");
        let english_text = "english transcript ".repeat(30);
        let japanese_text = "japanese transcript ".repeat(30);

        fs::write(
            &english,
            format!("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\n{english_text}\n"),
        )
        .expect("write english vtt");
        fs::write(
            &japanese,
            format!("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\n{japanese_text}\n"),
        )
        .expect("write japanese vtt");

        let candidate = load_best_transcript(
            &[english, japanese],
            Some("ja"),
            crate::constants::YOUTUBE_MIN_TRANSCRIPT_CHARS,
        )
        .expect("preferred transcript should load");
        assert_eq!(candidate.language.as_deref(), Some("ja"));
        assert_eq!(candidate.cues.len(), 1);
        assert_eq!(candidate.cues[0].text, japanese_text.trim());
    }

    #[test]
    fn load_best_transcript_falls_back_when_preferred_track_is_invalid() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let english = tempdir.path().join("video.en.vtt");
        let japanese = tempdir.path().join("video.ja.vtt");
        let japanese_text = "japanese transcript ".repeat(30);

        fs::write(
            &english,
            "WEBVTT\n\n00:aa:00.000 --> 00:00:02.000\nbroken english\n",
        )
        .expect("write invalid english vtt");
        fs::write(
            &japanese,
            format!("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\n{japanese_text}\n"),
        )
        .expect("write japanese vtt");

        let candidate = load_best_transcript(
            &[english, japanese],
            Some("en"),
            crate::constants::YOUTUBE_MIN_TRANSCRIPT_CHARS,
        )
        .expect("invalid preferred track should fall back");
        assert_eq!(candidate.language.as_deref(), Some("ja"));
        assert_eq!(candidate.cues.len(), 1);
        assert_eq!(candidate.cues[0].text, japanese_text.trim());
    }

    #[test]
    fn load_best_transcript_falls_back_when_preferred_track_is_too_short() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let english = tempdir.path().join("video.en.vtt");
        let japanese = tempdir.path().join("video.ja.vtt");
        let japanese_text = "japanese transcript ".repeat(30);

        fs::write(
            &english,
            "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nshort preferred\n",
        )
        .expect("write short english vtt");
        fs::write(
            &japanese,
            format!("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\n{japanese_text}\n"),
        )
        .expect("write japanese vtt");

        let candidate = load_best_transcript(
            &[english, japanese],
            Some("en"),
            crate::constants::YOUTUBE_MIN_TRANSCRIPT_CHARS,
        )
        .expect("short preferred track should fall back");
        assert_eq!(candidate.language.as_deref(), Some("ja"));
        assert_eq!(candidate.cues.len(), 1);
        assert_eq!(candidate.cues[0].text, japanese_text.trim());
    }

    #[test]
    fn subtitle_language_match_supports_regional_fallback() {
        let base_english = Path::new("/tmp/test123.en.vtt");
        assert!(subtitle_matches_language(base_english, Some("en-US")));

        let english_original = Path::new("/tmp/test123.en-orig.vtt");
        assert!(subtitle_matches_language(english_original, Some("en-US")));

        let base_french = Path::new("/tmp/test123.fr.vtt");
        assert!(subtitle_matches_language(base_french, Some("fr-FR")));

        let traditional_chinese = Path::new("/tmp/test123.zh-Hant.vtt");
        assert!(!subtitle_matches_language(
            traditional_chinese,
            Some("zh-Hans")
        ));
    }

    #[test]
    fn load_best_transcript_errors_when_all_candidates_are_invalid() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let english = tempdir.path().join("video.en.vtt");
        let japanese = tempdir.path().join("video.ja.vtt");

        fs::write(
            &english,
            "WEBVTT\n\n00:aa:00.000 --> 00:00:02.000\nbroken english\n",
        )
        .expect("write invalid english vtt");
        fs::write(
            &japanese,
            "WEBVTT\n\n00:bb:00.000 --> 00:00:02.000\nbroken japanese\n",
        )
        .expect("write invalid japanese vtt");

        let error = load_best_transcript(
            &[english, japanese],
            Some("en"),
            crate::constants::YOUTUBE_MIN_TRANSCRIPT_CHARS,
        )
        .expect_err("all invalid subtitle files should error");
        assert!(
            error.to_string().contains("timestamp")
                || error.to_string().contains("subtitle")
                || error.to_string().contains("invalid")
        );
    }
}
