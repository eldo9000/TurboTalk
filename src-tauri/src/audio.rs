// Microphone capture.
//
// Opens the default input device via cpal, resamples to 16kHz mono PCM
// (whisper's expected format), and writes a WAV file to a tempdir.
//
// Reference: Handy's audio module, sagascript's AVAudioEngine wrapper.
