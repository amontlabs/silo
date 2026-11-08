import { TooltipProvider } from '@/components/ui/tooltip';
import { ApplicationShell } from '@/features/application/components/application-shell';
import type { ApplicationSource } from '@/features/application/model/application-source';
import type { BackupArchive, BackupController } from '@/features/application/model/backup-source';
import { OverviewPage } from '@/features/application/pages/overview-page';
import { SettingsProvider } from '@/features/preferences/settings-store';
import { actions as applicationActions } from './fixtures';
import { showcaseSource } from './showcase-fixtures';

const noop = () => undefined;
const archive: BackupArchive = {
  name: 'Silo-Backup-1790474400.silo-backup',
  archivePath: '/Users/developer/Backups/Silo-Backup-1790474400.silo-backup',
  completedLabel: 'Just now',
  size: '1.2 GB',
  destination: '/Users/developer/Backups',
  computers: ['web'],
};

// Export operates on local computers. The showcase's remote lab must never appear as
// an export target, and this stopped computer does not imply a stop/restart cycle.
const source: ApplicationSource = {
  ...showcaseSource,
  devices: [],
  computers: showcaseSource.computers
    .filter(({ device, configuration }) => !device && configuration.name === 'web')
    .map(computer => ({ ...computer, state: 'stopped', stateDetail: 'Stopped' })),
  backup: { lastArchive: '', completedLabel: '', compressedSize: '', destination: archive.destination },
};

const actions: BackupController['actions'] = {
  chooseDestination: async () => null,
  chooseArchive: async () => null,
  inspectArchive: async selected => ({ archive: selected, valid: false, reason: 'Release video fixtures do not inspect archives.' }),
  startBackup: noop,
  exportAndVerify: async () => { throw new Error('Release video fixtures do not export archives.'); },
  startRestore: noop,
  cancelOperation: noop,
  dismissOperation: noop,
  revealArchive: async () => undefined,
  checkLogins: async () => ({ logins: [] }),
};

function backupAt(frame: number): BackupController {
  const complete = frame >= 95;
  return {
    actions,
    state: {
      snapshotId: 'release-backup-web',
      operationId: 'release-backup-web-operation',
      availability: 'available',
      destination: archive.destination,
      archives: complete ? [archive] : [],
      operation: {
        operation: 'backup',
        archive,
        runningNames: [],
        ...(complete ? {
          kind: 'result',
          outcome: 'success',
          title: 'Backup complete',
          message: 'Backup completed successfully.',
        } : {
          kind: 'running',
          progress: 0,
          indeterminate: true,
          // These are the current native controller's phase and detail. The
          // older application-backup fixture describes phases it does not emit.
          phases: [{ title: 'Capture and verify', detail: 'Silo is creating verified self-contained snapshots.', tone: 'running' }],
        }),
      },
    },
  };
}

export function ReleaseBackup({ frame }: { frame: number }) {
  // All displayed state comes from the render frame. Inert actions keep native
  // IO and the production success-dismiss timer from changing this movie.
  return <div className="r-product" style={{ width: 1060, height: 670 }}>
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
          {/* Export is on the computer page's checkpoints tab. */}
          <OverviewPage readOnly active={false} source={source} actions={applicationActions} backup={backupAt(frame)}
            onConfigurationsChange={noop} selectedComputerId={source.computers[0].configuration.id} computerTab="checkpoints" />
        </ApplicationShell>
      </TooltipProvider>
    </SettingsProvider>
  </div>;
}
