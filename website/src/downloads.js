const base = "https://github.com/amontlabs/silo/releases/latest/download/";

export function linuxDownloads(architecture) {
  if (!["x64", "arm64"].includes(architecture))
    throw new Error("Choose x64 or ARM64.");
  return {
    deb: `${base}Silo-linux-${architecture}.deb`,
    appImage: `${base}Silo-linux-${architecture}.AppImage`,
  };
}
