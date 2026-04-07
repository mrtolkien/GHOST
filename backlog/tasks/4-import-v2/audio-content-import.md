# Audio Content Import (Podcasts + YouTube Videos)

Add audio import support for podcasts and YouTube videos.

Podcast import shape is still open. YouTube video import v1 uses the two-step flow:

1. `ghost convert youtube --url <video-url>`
2. `ghost reference import <staging-dir> --topic <topic>`

Depends on the optional nix dependencies system being in place first.

## Podcast Import

Sources for transcripts (in priority order):

1. RSS `<podcast:transcript>` tag (rare, <1% adoption, but cleanest)
2. YouTube subtitles via yt-dlp (if podcast publishes on YouTube)
3. Website transcript scraping (many quality podcasts publish transcripts)
4. STT fallback: whisper.cpp on downloaded audio (~10 min/hr on CPU with small model)

Tools: yt-dlp (download + subtitles), whisper.cpp (STT), podcast-dl (RSS download)

## YouTube Video Import

Sources for transcripts (in priority order):

1. yt-dlp subtitle extraction (prefers manual subs, falls back to auto-generated)
2. whisper.cpp on downloaded audio (when captions missing or poor quality)

Speaker diarization (interviews): pyannote-audio, but requires HuggingFace token +
PyTorch. Skip for v1, revisit later.

## Note-Writing Flow

Same two-mode agent flow as book import:

- Mode A (autonomous): agent reads reference, creates source note + concept notes
- Mode B (guided): agent proposes notes, GHOST presents to user, user approves, agent
  creates approved notes

---

## Design: YouTube Video Import v1

### Scope

This design covers **single YouTube video URL import only**. Playlist import, channel
import, speaker diarization, and GPU-accelerated speech-to-text are explicitly out of
scope for v1.

The architecture should follow the existing two-step reference import flow already used
for EPUB, git, crawl, and PDF sources:

1. `ghost convert youtube` produces a staging directory with markdown transcript
   sections.
2. `ghost reference import <staging-dir> --topic <topic>` ingests the staging output
   into references/ and the database.
3. A dedicated `video-import` agent reads the imported references and creates notes.

This avoids a one-shot importer that mixes conversion, indexing, and note creation in a
single command.

### CLI Shape

Add a new converter command:

```sh
ghost convert youtube --url <video-url> [--output <dir>]
```

Behavior:

- Accepts only individual YouTube video URLs in v1.
- Resolves the video metadata and transcript source.
- Writes transcript sections as markdown files to a staging directory.
- Prints the staging path plus a short metadata summary to stdout.

`ghost reference import` remains unchanged and continues to be the only path that writes
references into the workspace and database.

### Transcript Acquisition

Transcript acquisition uses a strict priority order:

1. Human-created YouTube captions
2. Auto-generated YouTube captions
3. Audio-only download + CPU Whisper fallback

Notes:

- Auto-generated captions are acceptable for v1 and should be imported rather than
  forcing speech-to-text fallback.
- Speech-to-text is only used when captions are missing, not merely because captions are
  auto-generated.
- GPU acceleration is out of scope for v1. Whisper fallback uses CPU only.
- Rare external tools should follow the existing on-demand nix execution pattern rather
  than becoming permanent shell dependencies or long-lived services.

### Rare Tool Strategy

YouTube conversion should not require adding Whisper to `assets/shell/flake.nix`.
Instead, it should follow the same pattern already used for rare tools like `pdftoppm`:

- `yt-dlp` for subtitle extraction and audio-only download
- `ffmpeg` if audio normalization or format conversion is required
- `whisper.cpp` (CPU) only for fallback transcription when captions are absent

These tools should be invoked on demand via nix rather than treated as managed services.

### Staging Output

The staging directory contains only canonical markdown transcript content. It does
**not** preserve downloaded subtitle files or audio artifacts in `_originals/`.

The authoritative provenance anchor is the original YouTube URL, stored in
`_import.toml`, so the source can be re-fetched later if needed.

Each transcript section becomes a separate markdown file. Filenames must preserve stable
ordering and timestamp context, for example:

- `01-0000-intro.md`
- `02-0840-main-argument.md`
- `03-1735-conclusion.md`

Even if the slug portion is heuristic or absent, the ordering number and start timestamp
must be preserved.

### Section Splitting

Transcript sections should be split **primarily by length**, not only by uploader
chapters.

Rationale:

- Many videos have no uploader chapters.
- Some uploader chapters are badly balanced for later retrieval.
- GHOST may re-read individual references later, so section size should be bounded.

v1 uses a non-LLM hybrid splitter:

1. Start from timestamped transcript/caption blocks.
2. If uploader chapters exist, treat them as hints rather than hard boundaries.
3. Split oversized chapters by transcript length.
4. Merge tiny chapters/segments where needed.
5. If no chapters exist, split directly by transcript length using timestamp boundaries.

The converter should target relatively large transcript sections rather than tiny chunks.
The hard ceiling for a single reference file should be about **40,000 characters max**.
Staying under that limit keeps each reference comfortably readable and re-loadable for
modern LLM context windows while still avoiding pathological "entire 3-hour transcript
in one file" imports.

This design intentionally avoids semantic topic-boundary inference in v1.

### Metadata and Provenance

Video metadata belongs in `_import.toml`, not in a synthetic markdown file.

The YouTube import metadata should extend the existing provenance model with video
fields such as:

- `source_type = "youtube"`
- `source_url`
- `video_id`
- `title`
- `channel`
- `published_at`
- `duration_seconds`
- `transcript_source = "manual" | "auto" | "whisper"`
- `language`
- `section_count`
- `chapter_count`

This keeps markdown files content-only while preserving enough structured metadata for
re-import, updates, and debugging.

### Import Behavior

The generic `ghost reference import <staging-dir> --topic <topic>` path should remain
unchanged. The YouTube converter is responsible for producing staging output that fits
the existing importer contract:

- one or more markdown files
- optional `_import.toml` provenance
- no special-case import logic for YouTube

This preserves the clean separation between conversion and indexing that the new import
pipeline already uses.

### Note Extraction

Use a dedicated `video-import` agent rather than overloading `book-import`.

Reasoning:

- Video transcripts are structurally different from books.
- Timestamp-aware summaries and section-aware source notes matter more here.
- The transcript sections are shorter and more numerous than book chapters.

The `video-import` agent should:

1. Read all transcript section references for the topic.
2. Create a source note summarizing the video’s core thesis, structure, and notable
   timestamps when useful.
3. Create concept notes derived from the video.
4. Support the same two modes already intended for book import:
   - autonomous note creation
   - guided proposal then approval

### Failure Behavior

The command should fail clearly rather than importing junk.

Failure cases include:

- URL is not an individual YouTube video
- no usable captions are available and Whisper fallback fails
- transcript extraction succeeds but produces unusably short or malformed text

Errors should report which acquisition paths were attempted, for example:

- manual captions unavailable
- auto captions unavailable
- audio download failed
- Whisper transcription failed

This keeps operator debugging straightforward without requiring deep tracing inspection.

### Out of Scope

The following are deliberately excluded from v1:

- playlist import
- channel import
- GPU-accelerated Whisper
- speaker diarization
- semantic sectioning with an LLM
- preserving raw subtitle/audio artifacts in the workspace
