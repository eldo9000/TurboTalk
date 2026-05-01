// Recording state machine.
//
//   Ready ──hotkey-down──▶ Recording ──hotkey-up──▶ Transcribing ──done──▶ Ready
//
// One in-flight at a time. Hotkey events that arrive in the wrong state are dropped.
//
// Reference: typr's recorder.rs.
