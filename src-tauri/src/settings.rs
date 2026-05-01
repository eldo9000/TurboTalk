// Config persistence.
//
// Storage: ~/.config/librewin/turbotalk/config.toml
// (follows Libre convention; see librewin_common::config helpers)
//
// Schema:
//   [hotkey]       binding = "F1"  hold = true
//   [whisper]      model   = "small.en"
//   [cleanup]      mode    = "chaperone"  # off | regex | chaperone
//                  ollama_url = "http://localhost:11434"
//                  classifier_model = "llama3.2:3b"
//   [audio]        device  = "default"
