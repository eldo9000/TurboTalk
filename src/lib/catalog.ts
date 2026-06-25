export const KNOWN_FILENAMES = [
  'ggml-large-v3-turbo.bin',
  'ggml-large-v3-turbo-q5_0.bin',
  'ggml-large-v3.bin',
];

export function altModelVariant(m: { id: string }): string {
  return m.id.replace(/^parakeet-/, '');
}

export function altModelActive(m: { id: string }, cfgBackendVariant: string, cfgBackend: string): boolean {
  return altModelVariant(m) === (cfgBackendVariant || (cfgBackend === 'parakeet' ? 'tdt-0.6b-v2' : ''));
}
