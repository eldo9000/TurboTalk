// Chaperone Layer — LLM postprocessor.
//
// Pipes the raw whisper transcript through a small local LLM (Ollama at
// http://localhost:11434 by default) running as a CLASSIFIER, not a rewriter.
//
// Pipeline:
//   1. Classify the utterance into a mode: prose | code | command | raw
//   2. Route to a deterministic handler for that mode
//   3. Handler produces the final text
//
// Closed action space, open input space. The LLM never freely rewrites text —
// it picks among hand-written handlers. See:
//   ~/Downloads/Github/Business-OS/memory/project_chaperone_layer.md
//
// If Ollama is unreachable or cleanup mode is `off`, fall through to raw.
