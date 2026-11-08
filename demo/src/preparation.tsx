import { interpolate, delayRender, continueRender } from "remotion";
import { useLayoutEffect, useRef } from "react";
import type { ApplicationSource } from "@/features/application/model/application-source";
import {
  Check,
  KeyRound,
  Monitor,
  Laptop,
  Wifi,
  BatteryFull,
} from "lucide-react";
import { OverviewPage } from "@/features/application/pages/overview-page";
import { ApplicationShell } from "@/features/application/components/application-shell";
import { GitHubPage } from "@/features/application/pages/github-page";
import { SecretsPage } from "@/features/application/pages/secrets-page";
import { StatusBarContent } from "@/features/status-bar/status-bar";
import { SettingsProvider } from "@/features/preferences/settings-store";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SiloMark } from "@/components/silo-mark";
import type { BackupController } from "@/features/application/model/backup-source";
import { actions, demo, demoConfiguration, noop, sourceFor } from "./fixtures";
import { typed } from "./timeline";

export const ownerSource = () => ({
  ...sourceFor({ connected: false }),
  computers: [
    {
      ...demo,
      configuration: demoConfiguration,
      device: undefined,
      state: "stopped" as const,
      stateDetail: "Stopped",
    },
  ],
});
const archive = {
  name: "demo.silo",
  archivePath: "/Users/developer/Backups/demo.silo",
  completedLabel: "Just now",
  size: "1.2 GB",
  destination: "/Users/developer/Backups",
  computers: ["demo"],
};
function backupFor(frame: number): BackupController {
  const done = frame >= 130;
  return {
    state: {
      snapshotId: "demo",
      availability: "available",
      destination: archive.destination,
      archives: done ? [archive] : [],
      operation: {
        operation: "backup",
        archive,
        runningNames: [],
        ...(done
          ? {
              kind: "result" as const,
              outcome: "success" as const,
              title: "Backup complete",
              message: "demo is backed up.",
            }
          : {
              kind: "running" as const,
              progress: Math.min(98, Math.max(2, (frame - 30) * 1.05)),
              phases: [
                {
                  title: "Export computer disks",
                  detail: "demo · stopped",
                  tone:
                    frame >= 80 ? ("succeeded" as const) : ("running" as const),
                },
                {
                  title: "Compress archive",
                  detail: "demo.silo",
                  tone:
                    frame >= 80 ? ("running" as const) : ("waiting" as const),
                },
              ],
            }),
      },
    },
    actions: {
      chooseDestination: async () => null,
      chooseArchive: async () => null,
      inspectArchive: async (a) => ({ archive: a, valid: true }),
      startBackup: noop,
      exportAndVerify: async () => { throw new Error("Demo fixtures do not export archives."); },
      startRestore: noop,
      cancelOperation: noop,
      dismissOperation: noop,
      revealArchive: async () => undefined,
      checkLogins: async () => ({ logins: [] }),
    },
  };
}
// Open the shipping combobox through its normal focus handler. Its markup,
// filtering, option rows, and Radix portal all remain production-owned.
function RecordedGitHub({
  source,
  frame,
}: {
  source: ApplicationSource;
  frame: number;
}) {
  const root = useRef<HTMLDivElement>(null);
  const phase =
    frame < 10
      ? "idle"
      : frame < 45
        ? "choose"
        : frame < 110
          ? "read"
          : "write";
  useLayoutEffect(() => {
    const handle = delayRender(
      "Wait for the production repository popover layout",
    );
    if (phase === "choose")
      root.current
        ?.querySelector<HTMLInputElement>('[role="combobox"]')
        ?.focus({ preventScroll: true });
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => continueRender(handle));
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
      continueRender(handle);
    };
  }, [frame, phase]);
  return (
    <div ref={root} className="recorded-github">
      <GitHubPage key={phase} source={source} actions={actions} />
    </div>
  );
}
function RecordedSecret({
  source,
  frame,
}: {
  source: ApplicationSource;
  frame: number;
}) {
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (frame >= 150) return;
    const handle = delayRender("Open production secret editor");
    root.current
      ?.querySelector<HTMLButtonElement>('[aria-label="Edit SERVICE_TOKEN"]')
      ?.click();
    const first = requestAnimationFrame(() => continueRender(handle));
    return () => {
      cancelAnimationFrame(first);
      continueRender(handle);
    };
  }, [frame]);
  return (
    <div ref={root}>
      <SecretsPage source={source} onSaveSecret={noop} onRemoveSecret={noop} />
    </div>
  );
}
export function Preparation({
  page,
  frame,
}: {
  page: "github" | "secrets" | "backup";
  frame: number;
}) {
  const source = ownerSource();
  source.github = {
    state: "connected",
    account: "alex",
    accessEnabled: true,
    repositoryCatalog: ["acme/hello-silo", "acme/internal-tools"],
    deviceIdentity: { name: "Alex", email: "alex@example.com" },
    computers: [
      {
        computer: "demo",
        repositoryMode: "selected",
        allRepositoriesAllowChanges: false,
        identity: { name: "Alex", email: "alex@example.com", apply: true },
        repositories:
          frame >= 45
            ? [{ repository: "acme/hello-silo", allowPushes: frame >= 110 }]
            : [],
      },
    ],
  };
  source.secrets = [
    {
      id: "demo-token",
      name: "SERVICE_TOKEN",
      computers: ["demo"],
      allowedDomains: [
        page === "secrets" && frame < 110
          ? typed("api.example.com", frame, 15, 3)
          : "api.example.com",
      ],
      state: "active",
    },
  ];
  return (
    <div className="product-surface">
      <SettingsProvider initialSettings={source.preferences}>
        <TooltipProvider>
          <ApplicationShell
            activeTab={page === "backup" ? "computers" : page}
            computerSection="overview"
            settingsSection="general"
            systemIssueStatus={null}
            computerAttention={{ errors: 0, warnings: 0 }}
            onTabChange={noop}
            onComputerSectionChange={noop}
            onSettingsSectionChange={noop}
            canGoBack={false}
            canGoForward={false}
            onGoBack={noop}
            onGoForward={noop}
            reduceMotion
          >
            {page === "github" ? (
              <RecordedGitHub source={source} frame={frame} />
            ) : page === "secrets" ? (
              <RecordedSecret key={frame} source={source} frame={frame} />
            ) : (
              // Export is on the computer page's checkpoints tab.
              <OverviewPage readOnly active={false} source={source} actions={actions} backup={backupFor(frame)}
                onConfigurationsChange={noop} selectedComputerId={demoConfiguration.id} computerTab="checkpoints" />
            )}
          </ApplicationShell>
        </TooltipProvider>
      </SettingsProvider>
    </div>
  );
}
export function NativeDesktop({
  frame,
  owner = false,
  children,
}: {
  frame: number;
  owner?: boolean;
  children?: React.ReactNode;
}) {
  const source = owner
    ? ownerSource()
    : { ...sourceFor({ created: true }), computers: [demo] };
  if (owner)
    source.computers[0] = {
      ...source.computers[0],
      state: frame < 45 ? "stopped" : frame < 80 ? "starting" : "running",
      stateDetail: frame < 80 ? "Starting services" : "Running",
    };
  const statusActions = {
    ...actions,
    openSilo: noop,
    quit: noop,
    refresh: noop,
    openEditor: noop,
    openSite: noop,
    dismissRepositoryPush: noop,
  };
  return (
    <div className="native-desktop">
      <div className="mac-menubar">
        <span>●</span>
        <strong>{owner ? "Finder" : "Safari"}</strong>
        <span>File</span>
        <span>Edit</span>
        <span>View</span>
        <span>Window</span>
        <span>Help</span>
        <div className="menubar-right">
          <SiloMark style={{ width: 22, height: 22 }} />
          <Wifi size={20} />
          <BatteryFull size={23} />
          <span>Mon 10:24</span>
        </div>
      </div>
      {children}
      {owner && (
        <div className="app-stage wide-shot">
          <div className="app-scale">
            <div className="product-surface">
              <SettingsProvider initialSettings={source.preferences}>
                <TooltipProvider>
                  <ApplicationShell
                    activeTab="computers"
                    computerSection="overview"
                    settingsSection="general"
                    systemIssueStatus={null}
                    computerAttention={{ errors: 0, warnings: 0 }}
                    onTabChange={noop}
                    onComputerSectionChange={noop}
                    onSettingsSectionChange={noop}
                    canGoBack={false}
                    canGoForward={false}
                    onGoBack={noop}
                    onGoForward={noop}
                    reduceMotion
                  >
                    <OverviewPage
                      source={source}
                      actions={actions}
                      onConfigurationsChange={noop}
                    />
                  </ApplicationShell>
                </TooltipProvider>
              </SettingsProvider>
            </div>
          </div>
        </div>
      )}
      <div className="desktop-location">
        <Monitor size={19} />
        {owner ? "Office Mac" : "My laptop"}
      </div>
      {frame >= 20 && frame < 110 && (
        <div className="native-panel">
          <SettingsProvider initialSettings={source.preferences}>
            <TooltipProvider>
              <StatusBarContent
                source={source}
                actions={statusActions}
                focusContent={noop}
              />
            </TooltipProvider>
          </SettingsProvider>
        </div>
      )}
      {owner && frame >= 90 && (
        <div
          className="native-notification"
          style={{
            opacity: Math.min(1, (frame - 90) / 8),
            transform: `translateX(${Math.max(0, 110 - frame) * 12}px)`,
          }}
        >
          <div className="notification-icon">
            <SiloMark style={{ width: 30, height: 30 }} />
          </div>
          <div>
            <div className="notification-heading">
              Silo <span>now</span>
            </div>
            <strong>Computer status changed</strong>
            <p>demo: Running. Open Silo to review computer status.</p>
          </div>
        </div>
      )}
      <div
        className="native-cursor"
        style={{
          left: interpolate(
            frame,
            [0, 15, 35, 44, 65],
            [1673, 1673, owner ? 1698 : 1658, owner ? 1698 : 1658, 1810],
            { extrapolateLeft: "clamp", extrapolateRight: "clamp" },
          ),
          top: interpolate(
            frame,
            [0, 15, 35, 44, 65],
            [24, 24, 108, 108, 260],
            { extrapolateLeft: "clamp", extrapolateRight: "clamp" },
          ),
          opacity: frame < 70 ? 1 : 0,
        }}
      >
        <svg width="28" height="35" viewBox="0 0 28 35">
          <path
            d="M3 2v26l7-7 6 11 5-3-6-10h10L3 2Z"
            fill="#fff"
            stroke="#151718"
            strokeWidth="2"
            strokeLinejoin="round"
          />
        </svg>
      </div>
    </div>
  );
}

// A simultaneous, explicitly labelled view makes command and execution location visible.
export function RemoteHandoff({ frame }: { frame: number }) {
  const state = frame < 45 ? "stopped" : frame < 90 ? "starting" : "running";
  const remote = {
    ...demo,
    state,
    stateDetail:
      state === "starting"
        ? "Starting services"
        : state === "running"
          ? "Running"
          : "Stopped",
  } as typeof demo;
  const laptop = { ...sourceFor({ created: true }), computers: [remote] };
  const owner = {
    ...ownerSource(),
    computers: [{ ...remote, configuration: demoConfiguration, device: undefined }],
  };
  const statusActions = {
    ...actions,
    openSilo: noop,
    quit: noop,
    refresh: noop,
    openEditor: noop,
    openSite: noop,
    dismissRepositoryPush: noop,
  };
  return (
    <div className="handoff-scene">
      <div className="handoff-heading">Start here. Run there.</div>
      <div className="computer-pair">
        {[
          {
            label: "My laptop",
            detail: "Control it here",
            source: laptop,
            icon: "laptop",
          },
          {
            label: "Office Mac",
            detail: "It runs here",
            source: owner,
            icon: "desktop",
          },
        ].map(({ label, detail, source, icon }, i) => (
          <div className={`computer-device device-${icon}`} key={label}>
            <div className="computer-label">
              {i === 0 ? <Laptop size={25} /> : <Monitor size={25} />}
              <strong>{label}</strong>
              <span>{detail}</span>
            </div>
            <div className="computer-display">
              <div className="computer-menubar">
                <SiloMark style={{ width: 19, height: 19 }} />
                <span>Silo</span>
                <span className="computer-time">10:24</span>
              </div>
              <div className="computer-panel">
                <SettingsProvider initialSettings={source.preferences}>
                  <TooltipProvider>
                    <StatusBarContent
                      source={source}
                      actions={statusActions}
                      focusContent={noop}
                    />
                  </TooltipProvider>
                </SettingsProvider>
              </div>
              {i === 0 && frame < 50 && (
                <div
                  className="handoff-pointer"
                  style={{
                    left: 652,
                    top: interpolate(frame, [0, 25, 34], [150, 150, 110], {
                      extrapolateRight: "clamp",
                    }),
                  }}
                >
                  {frame >= 38 && frame < 45 && (
                    <span className="handoff-click" />
                  )}
                  <svg width="28" height="35" viewBox="0 0 28 35">
                    <path
                      d="M3 2v26l7-7 6 11 5-3-6-10h10L3 2Z"
                      fill="#fff"
                      stroke="#151718"
                      strokeWidth="2"
                    />
                  </svg>
                </div>
              )}
              {i === 1 && frame >= 103 && (
                <div
                  className="handoff-notification"
                  style={{
                    transform: `translateX(${Math.max(0, 111 - frame) * 12}px)`,
                    opacity: Math.min(1, (frame - 103) / 5),
                  }}
                >
                  <SiloMark style={{ width: 30, height: 30 }} />
                  <div>
                    <strong>Computer status changed</strong>
                    <p>demo: Running. Open Silo to review computer status.</p>
                  </div>
                </div>
              )}
            </div>
            <div className="computer-base" />
          </div>
        ))}
      </div>
      <div
        className="handoff-link"
        style={{ opacity: Math.min(1, Math.max(0, (frame - 42) / 8)) }}
      >
        <span>Start computer</span>
        <div>
          <i
            style={{
              width: `${interpolate(frame, [45, 90], [0, 100], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}%`,
            }}
          />
        </div>
        <span>
          {frame < 45
            ? "Office Mac connected"
            : frame < 90
              ? "Starting on Office Mac…"
              : "Running on Office Mac"}
        </span>
      </div>
    </div>
  );
}
