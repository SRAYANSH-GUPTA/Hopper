import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import {
  createManifest,
  readReleaseConfig,
  releasePlatforms,
  stageArtifacts,
  validateAppleEnvironment,
  validateSigningEnvironment,
  verifyUpdaterSignature,
} from "./release.mjs";

const repository = "SRAYANSH-GUPTA/Hopper";
const projectRoot = fileURLToPath(new URL("../", import.meta.url));

function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), "hopper-release-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

function signingKey() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const bytes = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const id = createHash("sha256").update(bytes).digest().subarray(0, 8);
  const packet = Buffer.concat([Buffer.from("Ed"), id, bytes]).toString("base64");
  return {
    publicKey: Buffer.from(`untrusted comment: test public key\n${packet}\n`).toString("base64"),
    signature(contents) {
      const signature = sign(null, createHash("blake2b512").update(contents).digest(), privateKey);
      const packet = Buffer.concat([Buffer.from("ED"), id, signature]).toString("base64");
      const comment = "timestamp:1\tfile:fixture";
      const commentSignature = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey).toString("base64");
      return Buffer.from(`untrusted comment: fixture\n${packet}\ntrusted comment: ${comment}\n${commentSignature}\n`).toString("base64");
    },
  };
}

function stagePlatform(root, config, key, platform) {
  const bundle = path.join(root, platform);
  mkdirSync(bundle);
  for (const extension of releasePlatforms[platform]) {
    const filename = path.join(bundle, `Hopper.${extension}`);
    const contents = Buffer.from(`fixture ${platform} ${extension}`);
    writeFileSync(filename, contents);
    if (["AppImage", "deb", "rpm", "app.tar.gz", "exe", "msi"].includes(extension)) {
      writeFileSync(`${filename}.sig`, key.signature(contents));
    }
  }
  stageArtifacts(config, platform, bundle, path.join(root, "release-artifacts"));
  return bundle;
}

test("repository configuration has matching versions and the correct updater endpoint", () => {
  const config = readReleaseConfig(projectRoot, repository);
  assert.equal(config.repository, repository);
  assert.throws(() => readReleaseConfig(projectRoot, "Dimillian/CodexMonitor"), /endpoint/);
  assert.throws(() => readReleaseConfig(projectRoot, ""), /owner\/repository/);
});

test("release refuses a version mismatch", (t) => {
  const root = fixture(t);
  mkdirSync(path.join(root, "src-tauri"));
  for (const filename of ["package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"]) {
    copyFileSync(path.join(projectRoot, filename), path.join(root, filename));
  }
  const pkg = JSON.parse(readFileSync(path.join(root, "package.json"), "utf8"));
  pkg.version = "0.0.0";
  writeFileSync(path.join(root, "package.json"), JSON.stringify(pkg));
  assert.throws(() => readReleaseConfig(root, repository), /versions differ/);
});

test("release requires explicit matching update key configuration", () => {
  const key = signingKey();
  assert.throws(() => validateSigningEnvironment(key, {}), /PRIVATE_KEY/);
  assert.throws(() => validateSigningEnvironment(key, { TAURI_SIGNING_PRIVATE_KEY: "configured" }), /PUBLIC_KEY/);
  assert.throws(() => validateSigningEnvironment(key, { TAURI_SIGNING_PRIVATE_KEY: "configured", TAURI_SIGNING_PUBLIC_KEY: "another key" }), /PUBLIC_KEY/);
  validateSigningEnvironment(key, { TAURI_SIGNING_PRIVATE_KEY: "configured", TAURI_SIGNING_PUBLIC_KEY: key.publicKey });
});

test("macOS release requires notarization credentials and a distribution identity", () => {
  const env = Object.fromEntries(["APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY", "APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"].map((name) => [name, "configured"]));
  assert.throws(() => validateAppleEnvironment({}), /APPLE_CERTIFICATE/);
  assert.throws(() => validateAppleEnvironment(env), /Developer ID/);
  env.APPLE_SIGNING_IDENTITY = "Developer ID Application: Test (TEAM)";
  validateAppleEnvironment(env);
  delete env.APPLE_PASSWORD;
  assert.throws(() => validateAppleEnvironment(env), /APPLE_PASSWORD/);
});

test("verifier accepts the upstream minisign-verify prehashed test vector", () => {
  // Same compatibility vector used by minisign-verify 0.2.4, the Tauri updater verifier.
  const publicKey = Buffer.from("untrusted comment: public key\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n").toString("base64");
  const signature = Buffer.from("untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==\n").toString("base64");
  verifyUpdaterSignature(Buffer.from("test"), signature, publicKey);
  assert.throws(() => verifyUpdaterSignature(Buffer.from("tampered"), signature, publicKey), /verification failed/);
});

test("verifier rejects wrong signing keys and tampered trusted comments", () => {
  const key = signingKey();
  const contents = Buffer.from("artifact");
  const signature = key.signature(contents);
  assert.throws(() => verifyUpdaterSignature(contents, signature, signingKey().publicKey), /different key/);
  const tampered = Buffer.from(Buffer.from(signature, "base64").toString().replace("file:fixture", "file:modified")).toString("base64");
  assert.throws(() => verifyUpdaterSignature(contents, tampered, key.publicKey), /verification failed/);
  assert.throws(() => verifyUpdaterSignature(contents, "not a signature", key.publicKey), /base64/);
});

test("complete release uses unique platform filenames and verifies every updater artifact", (t) => {
  const root = fixture(t);
  const key = signingKey();
  const config = { version: "1.2.3", repository, publicKey: key.publicKey };
  for (const platform of Object.keys(releasePlatforms)) stagePlatform(root, config, key, platform);
  const output = path.join(root, "release-artifacts");
  const manifest = createManifest(config, output, new Date("2026-01-01T00:00:00Z"));
  const installers = {
    "linux-x86_64": { appimage: "AppImage", deb: "deb", rpm: "rpm" },
    "linux-aarch64": { appimage: "AppImage", deb: "deb", rpm: "rpm" },
    "darwin-aarch64": { app: "app.tar.gz" },
    "darwin-x86_64": { app: "app.tar.gz" },
    "windows-x86_64": { nsis: "exe", msi: "msi" },
  };
  const expectedKeys = Object.entries(installers).flatMap(([platform, formats]) =>
    [platform, ...Object.keys(formats).map((installer) => `${platform}-${installer}`)]);
  assert.deepEqual(Object.keys(manifest.platforms).sort(), expectedKeys.sort());
  assert.equal(new Set(Object.values(manifest.platforms).map((platform) => platform.url)).size, 10);
  for (const metadata of Object.values(manifest.platforms)) {
    assert.ok(metadata.url.startsWith(`https://github.com/${repository}/releases/download/v1.2.3/`));
  }
  for (const [platform, formats] of Object.entries(installers)) {
    for (const [installer, extension] of Object.entries(formats)) {
      const metadata = manifest.platforms[`${platform}-${installer}`];
      assert.ok(metadata.url.endsWith(`Hopper_1.2.3_${platform}.${extension}`));
      verifyUpdaterSignature(Buffer.from(`fixture ${platform} ${extension}`), metadata.signature, key.publicKey);
    }
    const legacyInstaller = platform.startsWith("linux") ? "appimage" : platform.startsWith("windows") ? "nsis" : "app";
    assert.deepEqual(manifest.platforms[platform], manifest.platforms[`${platform}-${legacyInstaller}`]);
  }
  assert.ok(manifest.platforms["windows-x86_64"].url.endsWith(".exe"));
  assert.match(readFileSync(path.join(output, "SHA256SUMS"), "utf8"), /^[a-f0-9]{64}  /);
  writeFileSync(path.join(output, "Hopper_1.2.3_linux-x86_64.AppImage"), "tampered");
  assert.throws(() => createManifest(config, output), /verification failed/);
});

test("Linux package updates require signatures and reject modified Debian or RPM artifacts", (t) => {
  const root = fixture(t);
  const key = signingKey();
  const config = { version: "1.2.3", repository, publicKey: key.publicKey };
  let linuxBundle;
  for (const platform of Object.keys(releasePlatforms)) {
    const bundle = stagePlatform(root, config, key, platform);
    if (platform === "linux-x86_64") linuxBundle = bundle;
  }
  const output = path.join(root, "release-artifacts");
  for (const extension of ["deb", "rpm"]) {
    const artifact = path.join(output, `Hopper_1.2.3_linux-x86_64.${extension}`);
    const contents = readFileSync(artifact);
    writeFileSync(artifact, "tampered package");
    assert.throws(() => createManifest(config, output), /verification failed/);
    writeFileSync(artifact, contents);
    const signaturePath = path.join(linuxBundle, `Hopper.${extension}.sig`);
    const signature = readFileSync(signaturePath);
    rmSync(signaturePath);
    assert.throws(() => stageArtifacts(config, "linux-x86_64", linuxBundle, path.join(root, `unsigned-${extension}`)), /ENOENT/);
    writeFileSync(signaturePath, signature);
  }
});

test("release refuses missing platforms and ambiguous bundle outputs", (t) => {
  const root = fixture(t);
  const key = signingKey();
  const config = { version: "1.2.3", repository, publicKey: key.publicKey };
  const bundle = stagePlatform(root, config, key, "linux-x86_64");
  assert.throws(() => createManifest(config, path.join(root, "release-artifacts")), /Missing or empty.*linux-aarch64/);
  writeFileSync(path.join(bundle, "Another.AppImage"), "unexpected");
  assert.throws(() => stageArtifacts(config, "linux-x86_64", bundle, path.join(root, "another-output")), /exactly one/);
});

test("release refuses unsigned or empty bundles", (t) => {
  const root = fixture(t);
  const config = { version: "1.2.3", repository, publicKey: signingKey().publicKey };
  writeFileSync(path.join(root, "Hopper.AppImage"), "unsigned");
  assert.throws(() => stageArtifacts(config, "linux-x86_64", root, path.join(root, "output")), /ENOENT/);
  writeFileSync(path.join(root, "Hopper.AppImage"), "");
  assert.throws(() => stageArtifacts(config, "linux-x86_64", root, path.join(root, "output")), /Empty release artifact/);
});
