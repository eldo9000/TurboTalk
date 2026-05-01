// Active-application text injection.
//
// macOS path: write to clipboard (arboard), then send Cmd+V via osascript.
//   `osascript -e 'tell application "System Events" to keystroke "v" using command down'`
//
// Windows / Linux: enigo for synthetic key events.
//
// The clipboard's prior contents should be saved and restored after paste, so
// the user's clipboard isn't clobbered on every dictation.
