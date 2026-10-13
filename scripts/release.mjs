import { createHash, createPublicKey, verify } from "node:crypto";
import {
  appendFileSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const releasePlatforms = Object.freeze({
  "linux-x86_64": ["AppImage", "deb", "rpm"],
  "linux-aarch64": ["AppImage", "deb", "rpm"],
  "darwin-aarch64": ["app.tar.gz", "dmg"],
  "darwin-x86_64": ["app.tar.gz", "dmg"],
  "windows-x86_64": ["exe", "msi"],
});

function requireValue(value, message) {
  if (!value) throw new Error(message);
  return value;
}

function readJson(filename) {
  return JSON.parse(readFileSync(filename, "utf8"));
}

export function readReleaseConfig(root, repository) {
  requireValue(/^[\w.-]+\/[\w.-]+$/.test(repository ?? ""), "GITHUB_REPOSITORY must be owner/repository");
  const config = readJson(path.join(root, "src-tauri/tauri.conf.json"));
  const version = config.version;
  requireValue(/^\d+\.\d+\.\d+$/.test(version), "Release version must be a stable major.minor.patch");
  requireValue(config.productName === "Hopper", "Expected Hopper product name");
  const packageJson = readJson(path.join(root, "package.json"));
  const lock = readJson(path.join(root, "package-lock.json"));
  const cargo = readFileSync(path.join(root, "src-tauri/Cargo.toml"), "utf8");
  const cargoLock = readFileSync(path.join(root, "src-tauri/Cargo.lock"), "utf8");
  const versions = [packageJson.version, lock.version, lock.packages?.[""]?.version,
    cargo.match(/^version = "([^"]+)"/m)?.[1],
    cargoLock.match(/\[\[package\]\]\s+name = "hopper"\s+version = "([^"]+)"/)?.[1]];
  requireValue(versions.every((value) => value === version), "Release versions differ across package, Cargo, lockfiles, and Tauri config");
  const endpoint = `https://github.com/${repository}/releases/latest/download/latest.json`;
  requireValue(JSON.stringify(config.plugins?.updater?.endpoints) === JSON.stringify([endpoint]),
    "Updater endpoint must match the releasing GitHub repository");
  requireValue(config.bundle.createUpdaterArtifacts === true, "Tauri v2 updater artifacts must be enabled");
  requireValue(config.plugins.updater.pubkey?.trim(), "Updater public key is required");
  return { version, repository, publicKey: config.plugins.updater.pubkey.trim() };
}

export function validateSigningEnvironment(config, env) {
  requireValue(env.TAURI_SIGNING_PRIVATE_KEY?.trim(), "Configure the TAURI_SIGNING_PRIVATE_KEY release secret");
  requireValue(env.TAURI_SIGNING_PUBLIC_KEY?.trim() === config.publicKey,
    "Set TAURI_SIGNING_PUBLIC_KEY in release environment variables to the public key owned by this project; it must match tauri.conf.json");
}

export function validateAppleEnvironment(env) {
  for (const name of ["APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY", "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"]) {
    requireValue(env[name]?.trim(), `Configure ${name} for signed and notarized macOS releases`);
  }
  requireValue(env.APPLE_SIGNING_IDENTITY.startsWith("Developer ID Application:"),
    "macOS releases require a Developer ID Application identity");
}

function decodeBase64(value, label) {
  requireValue(typeof value === "string" && /^[A-Za-z0-9+/]+={0,2}$/.test(value), `Invalid ${label} base64`);
  const bytes = Buffer.from(value, "base64");
  requireValue(bytes.toString("base64") === value, `Invalid ${label} encoding`);
  return bytes;
}

// Tauri stores a base64-encoded Minisign envelope, not just an Ed25519 signature.
// Verify both the artifact hash and trusted comment using the app's shipped key.
export function verifyUpdaterSignature(contents, encodedSignature, encodedPublicKey) {
  const publicLines = decodeBase64(encodedPublicKey.trim(), "public key").toString("utf8").trim().split(/\r?\n/);
  const signatureLines = decodeBase64(encodedSignature.trim(), "signature").toString("utf8").trim().split(/\r?\n/);
  requireValue(publicLines.length === 2 && publicLines[0].startsWith("untrusted comment: "), "Invalid Minisign public key");
  requireValue(signatureLines.length === 4 && signatureLines[0].startsWith("untrusted comment: ") && signatureLines[2].startsWith("trusted comment: "), "Invalid Minisign signature envelope");
  const publicBytes = decodeBase64(publicLines[1], "public key packet");
  const signatureBytes = decodeBase64(signatureLines[1], "signature packet");
  const commentSignature = decodeBase64(signatureLines[3], "trusted comment signature");
  requireValue(publicBytes.length === 42 && publicBytes.subarray(0, 2).toString() === "Ed", "Invalid Minisign public key packet");
  requireValue(signatureBytes.length === 74 && commentSignature.length === 64, "Invalid Minisign signature size");
  requireValue(publicBytes.subarray(2, 10).equals(signatureBytes.subarray(2, 10)), "Updater signature belongs to a different key");
  // Modern Minisign uses Ed25519 over a BLAKE2b-512 hash. Reject legacy signatures.
  requireValue(signatureBytes.subarray(0, 2).toString() === "ED", "Updater signature must use prehashed Minisign");
  const key = createPublicKey({
    key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), publicBytes.subarray(10)]),
    format: "der",
    type: "spki",
  });
  const signature = signatureBytes.subarray(10);
  const hash = createHash("blake2b512").update(contents).digest();
  const comment = Buffer.from(signatureLines[2].slice("trusted comment: ".length));
  requireValue(verify(null, hash, key, signature) && verify(null, Buffer.concat([signature, comment]), key, commentSignature),
    "Updater signature verification failed");
}

// Installer names must match tauri-plugin-updater's OS-ARCH-INSTALLER lookup.
const updaterInstallers = Object.freeze({
  AppImage: "appimage",
  deb: "deb",
  rpm: "rpm",
  "app.tar.gz": "app",
  exe: "nsis",
  msi: "msi",
});

function artifactName(config, platform, extension) {
  return `Hopper_${config.version}_${platform}.${extension}`;
}

function allFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const filename = path.join(directory, entry.name);
    // The .app is already archived by Tauri; framework symlinks inside it are valid.
    if (entry.isDirectory()) return entry.name.endsWith(".app") ? [] : allFiles(filename);
    requireValue(entry.isFile(), `Unexpected non-file artifact: ${filename}`);
    return [filename];
  });
}

export function stageArtifacts(config, platform, bundleDirectory, outputDirectory) {
  const extensions = requireValue(releasePlatforms[platform], `Unsupported release platform: ${platform}`);
  const files = allFiles(bundleDirectory);
  mkdirSync(outputDirectory, { recursive: true });
  for (const extension of extensions) {
    const candidates = files.filter((file) => file.endsWith(`.${extension}`));
    requireValue(candidates.length === 1, `Expected exactly one .${extension} for ${platform}; found ${candidates.length}`);
    const source = candidates[0];
    requireValue(statSync(source).size > 0, `Empty release artifact: ${source}`);
    const destination = path.join(outputDirectory, artifactName(config, platform, extension));
    requireValue(!existsSync(destination), `Duplicate artifact: ${destination}`);
    if (Object.hasOwn(updaterInstallers, extension)) {
      const signature = readFileSync(`${source}.sig`, "utf8").trim();
      verifyUpdaterSignature(readFileSync(source), signature, config.publicKey);
      writeFileSync(`${destination}.sig`, `${signature}\n`);
    }
    copyFileSync(source, destination);
  }
}

export function createManifest(config, directory, now = new Date()) {
  const platforms = {};
  const expected = [];
  for (const [platform, extensions] of Object.entries(releasePlatforms)) {
    for (const extension of extensions) {
      const filename = artifactName(config, platform, extension);
      const artifact = path.join(directory, filename);
      requireValue(existsSync(artifact) && statSync(artifact).size > 0, `Missing or empty release artifact: ${filename}`);
      expected.push(filename);
      if (Object.hasOwn(updaterInstallers, extension)) {
        const signature = readFileSync(`${artifact}.sig`, "utf8").trim();
        verifyUpdaterSignature(readFileSync(artifact), signature, config.publicKey);
        expected.push(`${filename}.sig`);
        const metadata = {
          url: `https://github.com/${config.repository}/releases/download/v${config.version}/${encodeURIComponent(filename)}`,
          signature,
        };
        platforms[`${platform}-${updaterInstallers[extension]}`] = metadata;
        // Preserve the generic targets for clients predating bundle-specific lookup.
        if (["AppImage", "app.tar.gz", "exe"].includes(extension)) {
          platforms[platform] = metadata;
        }
      }
    }
  }
  const unexpected = readdirSync(directory).filter((name) => !expected.includes(name) && !["latest.json", "SHA256SUMS"].includes(name));
  requireValue(unexpected.length === 0, `Unexpected release artifacts: ${unexpected.join(", ")}`);
  const manifest = { version: config.version, pub_date: now.toISOString(), platforms };
  writeFileSync(path.join(directory, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  const checksums = [...expected, "latest.json"].sort().map((name) =>
    `${createHash("sha256").update(readFileSync(path.join(directory, name))).digest("hex")}  ${name}`);
  writeFileSync(path.join(directory, "SHA256SUMS"), `${checksums.join("\n")}\n`);
  return manifest;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = process.cwd();
    const config = readReleaseConfig(root, process.env.GITHUB_REPOSITORY);
    const [command, platform] = process.argv.slice(2);
    if (command === "preflight") validateSigningEnvironment(config, process.env);
    else if (command === "apple-preflight") validateAppleEnvironment(process.env);
    else if (command === "stage") stageArtifacts(config, platform, path.join(root, "src-tauri/target/release/bundle"), path.join(root, "release-artifacts"));
    else if (command === "manifest") {
      createManifest(config, path.join(root, "release-artifacts"));
      if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `version=${config.version}\n`);
    } else throw new Error("Usage: node scripts/release.mjs preflight|apple-preflight|stage PLATFORM|manifest");
    console.log(`Release ${command}: verified ${config.version}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
