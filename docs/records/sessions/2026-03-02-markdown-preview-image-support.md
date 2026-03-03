# Session: 2026-03-02 Markdown Preview Image Support

## Focus
Add image rendering support to markdown preview and fix relative path resolution.

## Problem
The markdown preview in the file viewer did not render images. The `renderInlineMd` function only handled links `[text](url)` but not image syntax `![alt](url)`. Additionally, relative image paths were not resolved correctly relative to the markdown file's directory.

## Solution
Enhanced the markdown rendering to support images with proper path resolution:

1. **Image syntax support**: Updated the regex pattern in `renderInlineMd` to recognize `![alt](url)` syntax
2. **Relative path resolution**: Added `basePath` parameter throughout the markdown rendering pipeline to resolve relative image paths relative to the markdown file's directory
3. **Path propagation**: Updated `mdToHtml`, `mdToHtmlCached`, `renderInlineMd`, and `updateFilePreview` to pass the current file path for proper resolution

### Path Resolution Logic
- **Absolute paths** (`/path/to/image.png`): Used as-is
- **URLs** (`https://example.com/image.png`): Used as-is
- **Relative paths** (`./images/pic.png` or `images/pic.png`): Resolved relative to the markdown file's directory

For example, if viewing `/root/docs/README.md`:
- `![logo](./images/logo.png)` → `/root/docs/images/logo.png`
- `![icon](../assets/icon.png)` → `/root/assets/icon.png`

## Changes Made

### `codoxear/static/app.js`

1. **Updated `renderInlineMd(s, basePath)`**:
   - Added `basePath` parameter
   - Updated regex to capture images: `!\[([^\]]*)\]\(([^)]+)\)`
   - Added relative path resolution logic for images
   - Renders images as `<img>` tags with proper `src` attribute

2. **Updated `mdToHtml(src, basePath)`**:
   - Added `basePath` parameter
   - Passed `basePath` to all `renderInlineMd()` calls

3. **Updated `mdToHtmlCached(src, basePath)`**:
   - Added `basePath` parameter
   - Updated cache key to include basePath: `src + "|" + basePath`

4. **Updated `updateFilePreview()`**:
   - Retrieves current file path from `filePathInput.value`
   - Passes file path to `mdToHtmlCached()`

5. **Updated helper functions**:
   - `renderList()`: Passes `basePath` to `renderInlineMd()`
   - `renderTable()`: Passes `basePath` to `renderInlineMd()`

## Benefits
- Markdown files with images now render correctly in preview mode
- Relative image paths work as expected
- Supports both local file paths and external URLs
- Maintains backward compatibility with existing link rendering

## Usage
When viewing a markdown file:
1. Click the "Preview markdown" button
2. Images referenced in the markdown will now render
3. Use relative paths like `![diagram](./diagrams/flow.png)` for images in subdirectories

## Testing
Manual testing:
1. Create a markdown file with various image references:
   ```markdown
   # Test Images

   Absolute path: ![logo](/root/logo.png)
   Relative path: ![icon](./images/icon.png)
   Parent directory: ![banner](../assets/banner.png)
   External URL: ![remote](https://example.com/image.png)
   ```
2. Open the file in the file viewer
3. Switch to preview mode
4. Verify all images render correctly
