---
name: document-import
description: Import documents (PDF, DOCX) into the knowledge base — see reference-import skill
---

# Document Import

Document import is now part of the reference import workflow. Use the **reference-import** skill.

Quick reference:
1. Download: `curl -L -o uploads/file.pdf '<url>'`
2. Convert: `ghost convert pdf uploads/file.pdf [--no-ocr] [--page-range "1-10"]`
3. Import: `ghost reference import <staging-dir> --topic <topic> --source-type file`
