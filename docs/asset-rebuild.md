# Preview asset updates
(authored by agents unless marked 🧑)

- mdBook 0.5 copies source assets into the build directory
    - the preview server serves that directory before the source directory
- source changes outside Markdown and configured asset changes trigger a full rebuild
    - replaces copied assets and preserves unsaved chapter overlays
    - Markdown chapters retain incremental patching
- configured additional CSS and JavaScript parent directories are watched without recursion
    - handles editors replacing files during atomic saves
- `mdbook_incremental_preview/tests/browser.cjs` replaces an SVG and checks the served bytes
