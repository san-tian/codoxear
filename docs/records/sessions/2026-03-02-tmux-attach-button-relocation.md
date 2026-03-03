# Session: 2026-03-02 Tmux Attach Button Relocation

## Focus
Move the tmux attach button from Session Tools modal to the topbar actions area for better accessibility.

## Problem
The tmux attach command was buried inside the Session Tools modal, requiring users to:
1. Click the Session Tools button
2. Scroll through the modal
3. Find the tmux attach row
4. Click the copy button

This made a frequently-used action unnecessarily difficult to access.

## Solution
Relocated the tmux attach functionality to a dedicated button in the topbar actions area, alongside other session actions (duplicate, rename, file, session tools, interrupt).

### Changes Made
- Added `tmuxAttachBtn` as a new icon button in the topbar actions
- Button shows terminal icon and is enabled only when the selected session has a tmux name
- Clicking the button copies the `tmux attach -t <name>` command to clipboard
- Removed the tmux attach row from Session Tools modal
- Updated `updateActionBtnState()` to manage tmux button state based on session's `tmux_name` property
- Changed Session Tools button icon from "terminal" to "more" to better reflect its purpose

## Benefits
- One-click access to tmux attach command
- Button is visible and accessible at all times (when a session with tmux is selected)
- Cleaner Session Tools modal with fewer rows
- Better visual hierarchy - frequently used actions in topbar, diagnostic tools in modal

## UI Changes
**Topbar actions (left to right):**
- Duplicate session
- Rename session
- View file
- **Tmux attach (new)**
- Session tools
- Interrupt

**Session Tools modal (simplified):**
- Status (SSH)
- Resume in TUI
- ~~TMUX attach~~ (removed)
- Live tail

## Files Modified
- `codoxear/static/app.js`:
  - Added `tmuxAttachBtn` button creation
  - Added button to topbar actions
  - Removed `sessionTmuxCmd` and `tmuxCopyBtn` from Session Tools
  - Updated `updateActionBtnState()` to manage tmux button state
  - Added `tmuxAttachBtn.onclick` handler
  - Removed tmux-related code from `updateSessionToolsContent()`
  - Changed Session Tools button icon from "terminal" to "more"

## Testing
Manual testing:
1. Select a web-owned session (has tmux)
   - Verify tmux attach button is enabled in topbar
   - Click button and verify command is copied to clipboard
   - Verify toast shows "tmux command copied"
2. Select a non-web session (no tmux)
   - Verify tmux attach button is disabled
   - Hover to see "Tmux attach not available" tooltip
3. Open Session Tools modal
   - Verify tmux attach row is no longer present
   - Verify Status and Resume rows still work correctly
