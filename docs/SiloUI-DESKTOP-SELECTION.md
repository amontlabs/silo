# Desktop and viewer selection

Assessment date: 2026-09-22. Scope: desktop environment, display architecture,
streaming server, browser viewer, and public agent interfaces. No constraint or
score comes from Luda. This is research and a selection recommendation, not a
claim that a replacement has passed Silo acceptance.

## Decision

**Select upstream noVNC as the preferred browser integration, with TurboVNC
3.3.1 as the first server to qualify. Keep Xfce/X11 as the present desktop
default. Do not describe either choice as the measured performance winner.**

The viewer recommendation changes the previous shortlist. TurboVNC has direct
WebSocket support, standard VNC interoperability, signed ARM64/AMD64 packages,
and an established noVNC integration. TigerVNC + noVNC remains the alternative
when distribution-managed packaging outweighs the additional WebSocket bridge.
KasmVNC remains the production baseline for the comparison, with useful
integrated capabilities and a narrower client/protocol boundary. A research
recommendation does not authorize silently replacing installed desktops.

Xfce is a policy choice among credible alternatives, not a unique factual
winner. Xfce, MATE and LXQt with a specified window manager have no established
ordering on Silo performance or reliability. Plasma is a serious challenger
for desktop programmability and UI design; GNOME is a serious challenger for
design, accessibility infrastructure and Ubuntu support continuity. Existing
installation cost and the current agent implementation earn no selection points.

**A complete, measured overall leaderboard cannot honestly be filled from the
available research.** The missing evidence is current, comparable runtime cost,
input correctness, recovery, human task completion and agent task completion on
the target stack. The rankings below distinguish documented selection priority
from those unmeasured outcomes. Assigning every product an invented “8/10 for
reliability” would recreate the original problem.

Evidence annexes: [desktops](research/desktop-selection-desktops-2026-09-22.md),
[viewers](research/desktop-selection-viewers-2026-09-22.md), and
[agent APIs](research/desktop-selection-agents-2026-09-22.md). Each contains
primary-source links and version boundaries.

## Product constraints and evidence notation

Repository inspection establishes Ubuntu 24.04, ARM64 and AMD64 recipes; a
1440×900 desktop with a 30 fps cap; a shared working-user session; local and
SSH-forwarded viewing; and Tauri webviews on macOS and Linux. CPU-only operation
is the required baseline because a usable guest GPU/encoder path has not been
established. This does not mean that the host has no GPU. See the
[recipe](../app/SiloUI/src-tauri/guest/setup-desktop.sh),
[viewer](../app/SiloUI/src-tauri/engine/src/desktop_viewer.rs), and
[proxy](../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs).

These constraints do **not** require X11, Xfce, VNC, a particular agent library,
or one vendor. A Wayland candidate is valid if its complete capture, input,
headless-session and browser-delivery path meets the same requirements.

| Mark | Meaning | Permitted inference |
| --- | --- | --- |
| **M** | Measured, with exact versions, configuration and method | Only the measured outcome and tested scope |
| **D** | Documented in primary sources | The documented capability exists in the identified version/configuration |
| **C** | Project performance/quality claim without a comparable reproducible trial | A hypothesis to test |
| **U** | Unknown, conflicting, or unqualified on the target | No positive or negative performance score |
| **P** | Product preference or decision rule | An explicit value judgment, never a measured fact |

Public measurements can be M without being transferable to Silo. A documented
API can be D while successful execution through Silo remains U. Desktop and
viewer scores must stay separate until a particular combination is qualified.

## Gates before weighted scores

A candidate cannot compensate for failing a gate by collecting feature points.
All new combinations still have untested gates; documented support is only the
entry ticket to qualification.

| Gate | Required result |
| --- | --- |
| Deployment | Headless operation without a guest GPU on both architectures; reproducible packages and startup; no mandatory Electron or cloud service |
| Session identity | Human and agent observe and operate the same intended user/session; reconnect preserves applications |
| Viewer compatibility | Actual WKWebView and WebKitGTK input, rendering, authentication, clipboard and scaling work through local and SSH transport |
| Input correctness | No lost, duplicated or stuck input in the declared acceptance suite; no unexpected clipboard reads/prompts |
| Access boundary | Authenticated private transport, scoped guest privileges and intentional clipboard/file access; no unsolicited public listener |
| Shipability | Stable selected release, compatible redistribution of the exact build, identified update owner and supported upgrade route |

The gates do not claim that a low version number is unstable. xrdp's maintained
0.10 stable line is eligible. A release explicitly called an RC is a different
case. License names alone do not settle redistribution of every bundled codec
or library; inventory the actual build before shipping it.

## Scoring the outcomes we actually care about

These weights are **P**, chosen for a shared development desktop. They express
priorities, not scientific constants. Freeze them before collecting results.
Report the dimensions as well as any total, and rerun the ordering after moving
each weight by ±25% and renormalizing. A winner that changes is preference-
dependent. Never normalize away U fields.

| Criterion | Weight | Measurement and scoring rule |
| --- | ---: | --- |
| Interaction performance and resources | 25 | Ten points for input-to-visible p95 latency, five each for total CPU time, memory and bandwidth at matched text/image quality. For each lower-is-better metric, normalized value = best eligible measurement / candidate measurement. Report raw units and spread. Memory means summed process PSS plus separately reported host VM/WebKit cost, not summed RSS. |
| Correctness and recovery | 25 | Correctness is a gate. After passing it: ten points for reconnect recovery, ten for crash/start/stop recovery, five for sustained-session continuity. Recovery uses success fraction, with failures and trial count published. Reconnection must preserve the same application state. |
| Human usability | 20 | Ten points for unassisted task completion fraction; five for error-free task fraction; five for successful-task time using the best/candidate ratio. Same tasks, representative new users, counterbalanced order. Distribution adoption and HIGs do not receive these points. |
| Agent effectiveness | 15 | Ten points for independently verified task success; five for successful-task time at equal success coverage. Same applications and public semantic/window/input capabilities, with retries and fallbacks reported. No advantage for today's agent adapter. |
| Maintenance and support | 10 | Two points each for: maintained stable line; documented security reporting; verifiable release/source artifacts; named ownership/issue process; a package support/update route through the product's declared support horizon. Missing evidence is U. Support horizon must be declared before this score can be completed. |
| Operability | 5 | One point each for documented headless lifecycle, machine-readable state/errors, connection diagnostics, controlled configuration, and documented recovery/upgrade procedure. Validate the exact shipped path. |

For complete data, `total = sum(weight × normalized value)`. Binary maintenance
and operability items use 0 or 1; an unknown item contributes an unresolved
interval `[0, weight]`, not zero. Scores are computed only among candidates
passing the gates. The performance ratio rewards measured relative efficiency;
absolute resource/latency budgets remain separate acceptance limits.

Normalize within the same host, architecture, network and workload cell; publish
those results separately before taking an equally weighted mean across the
predeclared cells. Use the median run value per cell and report variation.
Guest PSS diagnoses desktop cost; host VM and viewer memory measure host cost.
Do not add them together and double-count guest pages. Compare successful-task
times only at equal completion coverage. The qualified measured leaderboard
must survive plausible changes in workload weights as well as category weights.

**Current outcome leaderboard: unranked.** No candidate has the necessary
comparable data for the first 85 points. A total that ranks all candidates today
would be dominated by invented inputs. The documented shortlist below is still
useful for deciding what to build and test first.

## Desktop leaderboard by established capability

The table deliberately preserves ties. All six complete desktops can host GTK,
Qt and browser applications; that does not imply identical accessibility or
window behavior.

| Selection tier | Candidate | Established strengths | Material limitation |
| --- | --- | --- | --- |
| **A: CPU-only full-desktop shortlist, tied** | **Xfce 4.18/X11** | Modular desktop, optional compositing, documented settings CLI and conventional window-control interfaces | No comparable measurement establishes a RAM, latency, reliability or usability lead |
| **A: tied** | **MATE 1.26/X11** | Complete conventional desktop, optional compositing, GSettings and EWMH window control | Current component activity must be distinguished from old umbrella release dates and Noble patch delivery |
| **A: tied** | **LXQt 1.4 + Openbox/X11** | Modular desktop with explicit WM choice; optional separate compositor | LXQt alone is underspecified. Openbox's maintenance is part of this stack. Changing to Xfwm4/KWin creates a different candidate |
| **A: strongest richer-desktop challenger** | **Plasma 5.27/X11 on Noble; current Plasma/Wayland as a separate candidate** | Complete desktop, formal HIG and extensive KWin scripting; X11 compositor can be disabled | Qt Quick still needs rendering. Plasma 5 has no planned further releases except urgent/security cases; new long-lived deployment should assess supported Plasma 6/Wayland |
| **B: support/design challenger** | **GNOME 46 on Noble; current GNOME/Wayland separately** | Formal HIG, substantial accessibility infrastructure, standard Ubuntu Desktop support path; documented headless remote desktop | Compositor/session and remote-delivery path differ; no target efficiency result. Current GNOME's native X11 removal is a deployment change, not an agent incompatibility |
| **B: valid conventional alternative** | **Cinnamon 6.0/X11 on Noble** | Integrated conventional desktop and Linux Mint development ecosystem | No demonstrated target advantage over tier A; compositing is part of its shell. Mint's support promise does not automatically cover Silo's packages |
| **Separate product offer** | **i3 / Sway** | Explicit JSON IPC is particularly useful for window-tree queries and control | A window manager is not the full desktop offer. Assemble settings, launcher, files, policy prompts and accessibility before comparing footprint or novice UX |

Tier A does not claim four equal benchmark results; it means the evidence does
not eliminate or order these candidates on the dominant unknown outcomes.
Tier B is evaluation priority for this CPU-only appliance, not a claim that
GNOME/Cinnamon are worse desktops.

Capability sources include [Xfce's versioned compositor settings](https://docs.xfce.org/xfce/xfwm4/4.18/wmtweaks),
[MATE's pinned settings schema](https://github.com/mate-desktop/marco/blob/v1.26.2/src/org.mate.marco.gschema.xml),
[LXQt's WM choices](https://lxqt-project.org/wiki/Window-managers-%28X11%29.html),
[KWin scripting](https://develop.kde.org/docs/plasma/kwin/),
[GNOME HIG](https://developer.gnome.org/hig/), and [KDE HIG](https://develop.kde.org/hig/).

The practical decision is to retain Xfce while treating LXQt, MATE and Plasma
as real challengers. Retention avoids spending effort before a demonstrated
benefit; it is an explicit cost-of-change decision **outside** the quality score.
If selecting a fresh product primarily for documented desktop APIs and design
infrastructure, Plasma deserves the first prototype. “KDE is too heavy” is not
a supported reason to reject it.

## Viewer leaderboard for this product

This is an ordered **research recommendation**, based on the stated preference
for an independently embeddable client, replaceable server, stable software,
CPU operation and a small transport stack. It is not an FPS or crash-rate table.

| Priority | Complete candidate | Why it ranks here | What can reverse the choice |
| --- | --- | --- | --- |
| **1** | **TurboVNC 3.3.1 + upstream noVNC 1.7.0** | Public browser API, standard RFB/native-client alternatives, direct WebSocket support, mature release line and signed packages for both guest architectures | Narrow upstream ownership, bundled dependency patch responsibility, actual WebKit/input results, or worse quality-matched resource use |
| **2** | **TigerVNC 1.16.2 + noVNC 1.7.0 + websockify** | Same browser integration and standard protocol; broad distribution packaging | The extra bridge adds an operational component. If distribution-managed updates dominate, this becomes first; Noble's older TigerVNC package must not inherit 1.16.2's claims |
| **3** | **KasmVNC 1.5.0** | Integrated server/client/configuration; performance diagnostics, image/video modes, packaged Ubuntu builds | Modified protocol couples client and server; native standard VNC clients are not drop-in replacements; WebKit needs explicit qualification and supported clipboard behavior |
| **4, feature-dependent** | **Xpra + HTML5 client** | Full desktop and persistent individual applications, reconnection and richer desktop integrations | Greater feature/configuration surface to qualify; ranks first if individual Linux app windows become the actual product requirement |
| **5, protocol-dependent** | **xrdp + xorgxrdp + Guacamole** | Established RDP and documented browser gateway/client APIs | Additional gateway/protocol/runtime components; session routing must preserve the shared desktop. Ranks higher if RDP interoperability is required |
| **Experimental** | **Selkies 2.0.0rc1** | CPU/GPU paths, X11/headless Wayland, native packages and WebSocket delivery; serious media-feature candidate | Two-day-old RC at assessment date. Older Selkies/Webtop deployments cannot establish maturity of the changed 2.0 implementation. Stable-release gate is unresolved |

Sources: [TurboVNC 3.3.1](https://github.com/TurboVNC/turbovnc/releases/tag/3.3.1),
[TurboVNC guide](https://turbovnc.org/Documentation/Documentation),
[noVNC API](https://novnc.com/noVNC/docs/API.html),
[TigerVNC 1.16.2](https://github.com/TigerVNC/tigervnc/releases/tag/v1.16.2),
[KasmVNC 1.5.0](https://github.com/kasmtech/KasmVNC/releases/tag/v1.5.0),
[Xpra](https://github.com/Xpra-org/xpra),
[Guacamole architecture](https://guacamole.apache.org/doc/gug/guacamole-architecture.html),
[Selkies release](https://github.com/selkies-project/selkies/releases/tag/2.0.0rc1).

TurboVNC's WebSocket support is not a newly invented Silo bridge. It predates
the current release. Its Java native viewer is not required for a noVNC client;
an upstream package can still bundle viewer/JRE files, so inspect the installed
closure rather than asserting a Java-free package or a small footprint.

Its [support policy](https://turbovnc.org/Documentation/OSSupport) covers
Ubuntu LTS on AMD64/ARM64 with specified desktops and describes release
regression testing. Its [compatibility table](https://turbovnc.org/Documentation/Compatibility32)
lists Noble's Xfce 4.18 and MATE 1.26 without known issues and recommends a
non-compositing desktop when accelerated rendering is unavailable. This adds
concrete upstream support evidence for those pairings, not a Silo benchmark or
a claim that LXQt/Plasma cannot work.

This order is sensitive to product requirements. If audio, video, game input or
individual app forwarding becomes mandatory, rerank against those requirements;
plain noVNC does not automatically inherit every native VNC feature. Native
Sunshine/Moonlight and SPICE are different integration candidates, not drop-in
browser viewers for the existing virtual machine stack.

### WebKit changes the compatibility question

[noVNC 1.7 documents Safari support](https://github.com/novnc/noVNC/tree/v1.7.0#browser-requirements).
That is stronger relevant evidence than assuming every web client works in
WebKit, but it does not certify WKWebView or WebKitGTK in Silo.

[Kasm documents no direct Safari support](https://docs.kasmvnc.com/docs/clientside)
because of WebSocket Basic authentication. **Silo's native proxy already adds
that authentication header**, so this is not proof that Kasm cannot work here.
Separately, the [local input investigation](SiloUI-DESKTOP.md#native-viewer-input-investigation-2026-09-22)
identified a clipboard-detection mismatch and records an explicit seamless-off
fix. That defect does not prove the desktop is slow or that a replacement fixes
all typing/focus problems. Browser clipboard access remains subject to
[WebKit's user-interaction rules](https://webkit.org/blog/10855/async-clipboard-api/)
with every candidate.

## Agent convenience without favoring our implementation

| Need | Relevant interfaces | Selection consequence |
| --- | --- | --- |
| Understand controls and invoke actions | AT-SPI, exposed by GTK/Qt/Chromium applications | Test the same applications. Desktop branding cannot supply missing roles or actions |
| Screenshot and inject input | X11/XTest; Wayland ScreenCast/RemoteDesktop portals, PipeWire and libei/EIS | Both architectures have routes. Wayland requires compositor/backend and permission qualification; X11 permits broader cross-client access |
| Find, move and focus windows | EWMH; i3/Sway IPC; KWin scripting; compositor-specific bridges | i3/Sway have especially clear external IPC. KWin has a rich documented API. These do not reveal controls inside apps |
| Manage the browser connection | noVNC RFB object; other client/server APIs | Connection, resize, clipboard and key APIs are not a complete semantic agent API. noVNC's public reference does not promise an arbitrary programmatic pointer method |
| Work alongside a person | Explicit input ownership, cancellation and handoff | Neither XTest nor libei guarantees conflict-free simultaneous focus/pointer use |

Current [RemoteDesktop portal documentation](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
includes persistence and EIS. [GNOME Remote Desktop](https://github.com/GNOME/gnome-remote-desktop/blob/main/README.md)
documents headless modes. Therefore Wayland is eligible. Qualify exact packages:
current upstream capabilities are not automatically present in Ubuntu 24.04.
The [agent annex](research/desktop-selection-agents-2026-09-22.md) records
toolkit activation, unattended operation and compositor differences.

## What measured performance currently tells us

| Evidence | Actual observation | Selection value |
| --- | --- | --- |
| [Fedora author's 2019 comparison](https://fedoramagazine.org/fedora-desktops-memory-footprints/) | Default Fedora 31 VMs reported LXQt 391 MB, Xfce 448, MATE 465, GNOME 612, Cinnamon 624, Plasma 733 | Historical evidence only. Different services, old desktops, no streaming; not a 2026 leaderboard |
| [Kasm's own software-mode benchmark](https://docs.kasm.com/docs/develop/reference/kasm-performance) | 1080p video on EPYC 7J13, medium/24 fps cap: H.264 74% server CPU, 21 fps, 14.35 Mbit/s; JPEG/WebP 64%, 21 fps, 33.48 Mbit/s; H.265 230%, 13.2 fps, 6.53 Mbit/s | Encoding trades CPU, delivered frames and bandwidth. Vendor CPU convention, unmatched quality and no competing server/WebKit/ARM64 trial prevent extrapolation |
| [Silo's September 18 observation](SiloUI-DESKTOP.md#verification-2026-09-18) | Warm stop/start without a viewer recorded about 123 MiB less guest MemAvailable and 233 MiB desktop-user PSS | Two different memory quantities; neither measures host cost or active streaming, and no alternative was tested |

TurboVNC also publishes [first-hand encoding research](https://turbovnc.org/About/Reports),
but those reports concern 2008/2011 implementations. Its newer documentation of
optimization mechanisms is useful architecture evidence, not a comparative
2026 result. Xfce's [4.20 requirements page](https://docs.xfce.org/xfce/4.20/system-requirements)
itself includes an Ubuntu 19.10/Xfce 4.14-era memory test. A current page heading
does not make the measurements current.

## Maintenance, dependencies and the proposed checklist

All major candidates have genuine release and deployment histories. Recent
activity is necessary evidence; it does not establish maintainer redundancy,
funding resilience or comparative failure rates. noVNC names a small core team;
TurboVNC has concentrated ownership; Kasm is vendor-led; GNOME/KDE have nonprofit
governance. None of those facts alone proves better bug response. TigerVNC
1.16.2 corrected an omitted security fix in 1.16.1: a mature project's release
still needs version-specific verification.

**The near-term maintenance issue is the guest baseline.** Xubuntu, Lubuntu,
Kubuntu and Ubuntu MATE 24.04 advertise support through April 2027. That is not
the same commitment as Ubuntu's core support, nor proof every Silo package loses
coverage on that date. Identify the exact desktop package security/update route.
Plasma 5's upstream status and the supported Wayland direction make a new
long-term commitment to Plasma 5/X11 particularly unattractive. Sources and
package versions are in the [desktop annex](research/desktop-selection-desktops-2026-09-22.md).

| Checklist item | Decision rule |
| --- | --- |
| Active maintainers, thoughtful APIs, good docs | Keep; inspect stable releases, issue ownership, public contracts, versioning and recovery documentation |
| Fast, small, no giant dependency tree | Keep as measurements: installed/download bytes, running processes, memory/CPU and security-update ownership. Count bundled libraries too |
| 2–5 years old, post-1.0, survived a rewrite | Reject as gates. Age and version style do not measure correctness; a rewrite resets relevant production evidence |
| Praise, fast growth, not the most popular | Reject. Adoption is deployment evidence; popularity and endorsements do not measure task success |
| No acquisition or previous abandonment | Investigate present ownership, update delivery and forkability; do not infer present fitness from history alone |
| Repeated crash PRs, absent maintainer, undocumented API | Investigate exact affected versions and unresolved failure mechanisms; raw issue totals are not failure rates |
| No unexplained Electron requirement | Keep. The preferred browser stacks do not require adding Electron |

Do not equate upstream source size with installed size, package count with
attack surface, or a Java/FFmpeg/Python dependency with poor engineering.
Configuration breadth, vendored forks, X-server fixes and codec licensing create
specific maintenance work that an exact package inventory can expose.

## Experiment that completes the decision

This work did not install candidate desktops, connect to live guests or launch
a Silo bundle. All target acceptance and comparative performance fields remain
unmeasured. The following is the declared next experiment, not a reported result.

1. Freeze the app workload, package manifests, supported host WebKit versions,
   guest CPU/RAM, 1440×900 resolution, 30 fps cap, locale and quality constraints.
   Use native ARM64 and AMD64 separately. Keep identical application versions and
   equal functionality; do not compare a full distribution image with a stripped
   desktop. Record guest rendering and host decoding acceleration separately.
2. Compare corrected KasmVNC, TurboVNC/noVNC and TigerVNC/noVNC on the same Xfce
   desktop. Exercise actual packaged WebKit, local transport and SSH with fixed
   bandwidth/RTT/loss profiles. Record input-to-guest and input-to-visible timing
   separately so encoding is not blamed for input delivery failures.
3. Require zero lost/duplicate/stuck events across 10,000 independently checked
   input actions, 100 focus/clipboard transitions and 100 reconnects per tested
   configuration. Include US/French layouts, accented text, IME, drag/release,
   modifiers, explicit paste, scaling and host sleep/resume. Run an eight-hour
   mixed-workload soak. Zero observed failures is not proof of zero failure rate.
4. Compare Xfce, LXQt+Openbox, MATE and Plasma with the same qualified stream and
   application set. Test GNOME and current Plasma/Wayland as complete separate
   session/capture/viewer combinations. Include their real integration costs;
   do not transfer an X11 result to a Wayland session.
5. Run fixed human tasks and independent agent tasks: launch an app, find/edit a
   file, handle a dialog, switch windows, change a setting and recover a session.
   Include GTK3/4, Qt5/6 and browser/Electron apps. Verify outcomes, accessible
   controls, retries and focus interference. Small user pilots discover problems;
   they do not establish a population-wide usability ranking.
6. Start with five randomized performance runs per cell, publish distributions,
   and extend noisy/close comparisons. Require a median improvement of at least
   20% in one priority cost or task-time metric, without more than 10% regression
   in the others, before resource efficiency alone triggers migration. These
   margins are proposed product rules, not external performance facts. Correctness
   fixes can justify migration independently. Do not change rules after results.

The next action is the isolated **Xfce: corrected KasmVNC versus
TurboVNC/noVNC versus TigerVNC/noVNC** comparison using one identical input and
workload harness. It tests the preferred viewer architecture without crediting
the incumbent desktop with a performance victory it has not earned.
