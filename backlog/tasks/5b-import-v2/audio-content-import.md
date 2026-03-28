# Audio Content Import (Podcasts + YouTube Videos)

Add `ghost reference import podcast` and `ghost reference import video` commands.

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
