# Build Silo from source

Use this path if you want to own the GitHub connection as well as run Silo locally. You create a GitHub App under your account, put its configuration in an ignored local file, and build Silo with it. To install a prebuilt app instead, follow the [README](../README.md#install). Silo's publisher has no ownership of that registration or independent installation access through it. You still need to trust the code you build and run.

You do not need to host a server, create a personal access token, or generate an App private key. Silo talks directly to GitHub and stores user credentials in macOS Keychain or Linux Secret Service. Native builds currently require GitHub App configuration even if you plan to skip GitHub during onboarding.

## 1. Install the build tools

Build for the same operating system and architecture where you will run Silo: Apple Silicon macOS 14+, or x86-64/ARM64 Linux compatible with Ubuntu 24.04. Cross-compilation is not covered here.

Install [Node.js 24](https://nodejs.org/en/download), Python 3.11 or newer, [Go 1.25 or newer](https://go.dev/doc/install), Git, and [Rust through rustup](https://rustup.rs/). Install the pinned Rust toolchain used by the bundled VM runtime:

```sh
rustup toolchain install 1.94.0 --profile minimal
```

**macOS:** install Apple's command-line tools if you do not already have them:

```sh
xcode-select --install
```

**Ubuntu 24.04:** install the native build and packaging dependencies:

```sh
sudo apt update
sudo apt install -y build-essential pkg-config libwebkit2gtk-4.1-dev \
  libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
  patchelf libdbus-1-dev libclang-dev libcap-ng-dev cmake \
  libfuse2t64 squashfs-tools file gstreamer1.0-libav gstreamer1.0-plugins-base
```

Linux Debian packages declare `gstreamer1.0-libav`, `gstreamer1.0-plugins-base` (the Opus decoder behind desktop sound) and `openssh-client` as runtime dependencies (RPM packages declare `gstreamer1-plugins-base` and `openssh-clients`): the desktop viewer and editor handoff run `ssh` and `ssh-keygen`. AppImages bundle the GStreamer media framework from the Ubuntu build host, including the installed H.264 decoder and Opus plugins (confirm `libgstopus` is in a built AppImage), but use the system OpenSSH client.

Linux needs a working desktop credential store implementing Secret Service, such as GNOME Keyring, for GitHub login. Running local computers also requires hardware virtualization and access to `/dev/kvm`; compilation alone does not establish that KVM works. See [Tauri's platform prerequisites](https://v2.tauri.app/start/prerequisites/) for OS setup details.

## 2. Get the source

```sh
git clone https://github.com/amontlabs/silo.git
cd silo
rustup override set 1.94.0
npm --prefix app/SiloUI ci
```

`app/SiloUI/src-tauri/rust-toolchain.toml` pins the same toolchain for Cargo and Tauri commands run inside `src-tauri`; the override covers commands run from the repository root. Run the remaining commands from this `silo` directory. The first native build downloads and compiles the bundled runtime and prepares the guest image, so it needs internet access and can take substantially longer than later builds.

## 3. Create your GitHub App

Open [GitHub → Settings → Developer settings → GitHub Apps → New GitHub App](https://github.com/settings/apps/new). This guide assumes you are using your personal account and repositories.

Fill in these settings:

| GitHub field | What to enter |
| --- | --- |
| GitHub App name | A unique name, such as `Silo-yourusername-dev`. |
| Homepage URL | `https://github.com/amontlabs/silo` or your own fork's URL. |
| Callback URL / Redirect URI | `http://127.0.0.1/github/callback` |
| Allow wildcard matching, if shown | Leave disabled. |
| Expire user authorization tokens | Leave enabled; Silo renews them automatically. Check **Optional features** after creation if this setting is not on the form. |
| Request user authorization (OAuth) during installation | Leave disabled; Silo starts authorization itself. |
| Enable Device Flow | Leave disabled. |
| Setup URL | Leave empty. |
| Webhook → Active | Uncheck it; leave the webhook URL empty. |
| Where can this GitHub App be installed? | **Only on this account** for your personal repositories. |

The callback is a temporary listener on the device running Silo. Silo supplies its port when opening the browser; do not copy a port or authorization URL from a previous login attempt into the registration. This portless registration was exercised in Silo's [live OAuth audit](SiloUI-OAUTH-RELEASE-AUDIT.md#live-pkce-reproduction).

Under **Repository permissions**, choose the access you need:

| What you want to do | Permission |
| --- | --- |
| Clone and read repository code | **Contents: Read-only** |
| Also push commits | **Contents: Read & write** instead |
| Push changes to GitHub Actions workflow files | Also **Workflows: Read & write** |
| Work with issues or pull requests | Also **Issues** and/or **Pull requests**, with Read-only or Read & write as needed |
| Use other GitHub features from a computer | Enable their corresponding repository permissions, such as Actions or Packages. |

Leave **Account**, **Organization**, and **Enterprise** permissions at **No access**. Silo currently accepts repository permissions only. GitHub includes mandatory read-only Metadata access. The App's permissions are the maximum Silo can grant: each computer starts read-only, and enabling changes in Silo cannot exceed what you approved here.

Click **Create GitHub App**. Creating it does not yet install it on your repositories; Silo will guide you through installation when you connect. GitHub's [registration guide](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app) explains the form in more detail.

**Organization repositories or other users:** a private personal App only works for its owner and cannot be installed on another account. To use the same registration on organization repositories, choose **Any account** and obtain any required organization approval. This makes the App installable by others, not their repositories public. You still own the App and its credentials. Teams can instead register it under their organization, subject to its policies.

## 4. Add your App configuration to Silo

On your new App's **General** settings page, find these three values:

| Silo setting | Where to find it |
| --- | --- |
| `SILO_GITHUB_APP_SLUG` | The last part of the App settings URL. For `github.com/settings/apps/silo-alex-dev`, use `silo-alex-dev`. |
| `SILO_GITHUB_CLIENT_ID` | **Client ID**, not the numeric App ID. |
| `SILO_GITHUB_CLIENT_SECRET` | Click **Generate a new client secret**, then copy the generated value. This is not an App private key. |

Create the configuration file once:

```sh
cp app/SiloUI/github-build.example.json app/SiloUI/github-build.local.json
chmod 600 app/SiloUI/github-build.local.json
```

Open `app/SiloUI/github-build.local.json` in your editor and replace **all three values**, including the example's existing App slug and client ID:

```json
{
  "SILO_GITHUB_APP_SLUG": "silo-yourusername-dev",
  "SILO_GITHUB_CLIENT_ID": "YOUR_CLIENT_ID",
  "SILO_GITHUB_CLIENT_SECRET": "YOUR_CLIENT_SECRET"
}
```

Keep the quotes and use your actual values. The file is ignored by Git; do not commit or share it. Silo reads it automatically when compiling. Previously exported `SILO_GITHUB_*` environment variables override the file, so unset those if you configured a different App earlier.

The client secret is embedded in your compiled app and can be extracted from it. Keep this build for your own use; do not treat that embedded value as a confidential credential. No App private key is needed or bundled. See the [credential audit](SiloUI-OAUTH-RELEASE-AUDIT.md) for the precise security model.

## 5. Run or build Silo

To run the native app while developing:

```sh
npm --prefix app/SiloUI run desktop
```

This prepares the runtime and opens Silo Dev, a separate build channel (`org.silo.dev`) with its own data, Keychain items and Connections identity, so it never touches an installed Silo. `npm --prefix app/SiloUI run dev:import-production-settings` copies your installed Silo's configuration (not computers) into it; see [build channels](SiloUI-BUILD-CHANNELS.md). Keep the terminal running. `npm --prefix app/SiloUI run dev` starts only the frontend server. The main page requires the native app; it cannot run computers or complete GitHub setup in a browser. For an interactive browser demo with sample data, use the [website preview](../website/README.md#run).

For an optimized app you can launch without the development terminal, use the command for your platform. These are production-channel builds (`org.silo.preview`) and share data with an installed Silo. These commands disable updater artifact signing, so you do not need the project's release signing keys.

**macOS:**

```sh
npm --prefix app/SiloUI run desktop:build
open app/SiloUI/src-tauri/target/release/bundle/macos/Silo.app
```

The macOS command applies and verifies the VM helper's exact-engine ad-hoc
signature before succeeding. It creates a local app without a DMG or updater
archive; no Apple distribution certificate is required.

**Linux:**

```sh
npm --prefix app/SiloUI run desktop:build -- --bundles appimage \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

Open `app/SiloUI/src-tauri/target/release/bundle/appimage/` and launch the generated `.AppImage` for your architecture. If necessary, enable **Allow executing file as program** in its file properties. Use this AppImage for a personal source build rather than enrolling it in Silo's official Debian update source.

## 6. Connect and use your repositories

1. In the app you just built, choose **Connect GitHub** during setup or from its GitHub settings.
2. In the browser on that same device, sign in with the account that owns your private App. Confirm that GitHub shows **your App's name**.
3. Authorize it. If it is not installed yet, Silo opens the installation page; choose **Only select repositories** and select the repositories you want available.
4. Return to Silo. Choose repositories for each computer, then enable **Allow GitHub changes** only where needed. Clone over HTTPS inside the computer; Silo supplies credentials automatically.

Development builds (`npm run desktop`, `desktop:build:debug`, or any `--debug`
build) use **Silo Dev** (`org.silo.dev`) with separate computers, settings and credentials.
Optimized production-channel builds use **Silo** (`org.silo.preview`) and share
data with official builds. If you use that channel and previously connected an
official build, disconnect that connection before connecting your own App.
Before launching another production build, quit the existing Silo instance
safely; Quit stops its local computers. See [build channels](SiloUI-BUILD-CHANNELS.md).

## Updating and troubleshooting your build

**Update by rebuilding:** pull new source, install its dependencies, and repeat your build command. Keep `github-build.local.json` in place. Resolve any local source changes before pulling.

```sh
git pull --ff-only
npm --prefix app/SiloUI ci
```

Do not install an official Silo update over this build: it would replace your compiled GitHub App configuration with the publisher's. Turn off **Automatically check for updates** in Silo's Updates settings. Disabling updater artifact signing during the build does not disable update checks.

| Problem | What to check |
| --- | --- |
| Build reports missing or invalid GitHub configuration | Check all three values in `github-build.local.json`, including accidental whitespace and old environment overrides. Rebuild after editing. |
| GitHub authorization page returns 404 | Check the compiled client ID and App visibility. A private personal App only allows its owner to sign in. |
| Browser cannot return to Silo | Keep Silo running, use the browser on the same device, check the callback setting above, then cancel and start a fresh connection. |
| Repository is missing | Add it to your App installation in GitHub settings; organization approval may be required. Refresh the repository list in Silo. |
| Reads work but writes fail | Check both the GitHub App's permissions and that computer's **Allow GitHub changes** setting. Approve updated installation permissions on GitHub if you changed them. |

The React/TypeScript frontend and Rust/Tauri backend live in [`app/SiloUI`](../app/SiloUI). For tests, distribution signing, and maintainer releases, see the [development and release guide](SiloUI-RELEASES.md). Browse the [documentation index](README.md) for implementation details.
