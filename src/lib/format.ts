export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const mb = n / (1024 * 1024);
  return mb < 1024 ? `${mb.toFixed(1)} MiB` : `${(mb / 1024).toFixed(2)} GiB`;
}

export function formatMtime(epochSeconds: number | null): string {
  if (epochSeconds == null) return "unknown mtime";
  return new Date(epochSeconds * 1000).toLocaleString();
}
