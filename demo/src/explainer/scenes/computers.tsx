import React from "react";
import {
  AppWindow,
  Bot,
  Cpu,
  Folder,
  HardDrive,
  Laptop,
  Monitor,
  MousePointerClick,
  SquareTerminal,
} from "lucide-react";
import { Chip, Chips, Edge, Group, List, Node, Text } from "../kit.tsx";
import type { Visual } from "./index.ts";

const linuxStack: Visual = {
  render: () => (
    <>
      <Node id="silod" x={160} y={200} w={660} h={84} mono size={28} title="silod" icon={Cpu} tone="engine" at="msb" />
      <Node id="msb" x={160} y={326} w={660} h={90} mono title="msb · MicroSandbox 0.7.6" sub="one process per computer" at="msb+20" hot="msb patches" />
      <Node id="krun" x={160} y={458} w={660} h={90} mono title="libkrun" sub="virtual machine monitor + its own Linux kernel" at="krun" />
      <Node id="hv" x={160} y={590} w={660} h={90} title="Hypervisor.framework" sub="on a Linux server: KVM" tone="device" at="hv" />
      <Edge from="silod" to="msb" at="msb+24" />
      <Edge from="msb" to="krun" at="krun+6" />
      <Edge from="krun" to="hv" at="hv+6" />
      <Node x={160} y={740} w={660} h={110} title="24 Silo patches, pinned by checksum" sub={"live port publishing · secrets on standard input ·\nusage events · restore fixes"} tone="engine" at="patches" />

      <Group id="vm" x={980} y={220} w={850} h={330} label="The computer" tone="guest" solid at="own" />
      <Node x={1030} y={280} w={370} h={90} title="Its own kernel" icon={Cpu} tone="guest" at="own" />
      <Node x={1430} y={280} w={350} h={90} title="Root disk" icon={HardDrive} tone="guest" at="own+45" hot="own" />
      <Node x={1030} y={410} w={370} h={90} title="Agent, as root" icon={Bot} tone="guest" at="own" />
      <Node x={1430} y={410} w={350} h={90} title="Workspace disk" icon={HardDrive} tone="guest" at="own+75" hot="own" />
      <Node id="folders" x={1030} y={690} w={370} h={90} title="Your folders" icon={Folder} tone="device" at="nofolder" />
      <Edge from="folders.t" to={{ x: 1215, y: 550, side: "b" }} blocked label="never shared" at="nofolder+8" />
      <Node id="share" x={1430} y={690} w={350} h={90} mono title="/.msb" sub="control folder, read-write" tone="bad" at="share" />
      <Edge from="share.t" to={{ x: 1605, y: 550, side: "b" }} tone="bad" both at="share+8" />
      <Text x={1430} y={796} w={400} size={22} tone="bad" at="share+90">
        not yet security reviewed
      </Text>
    </>
  ),
};

const image: Visual = {
  render: () => (
    <>
      <Node x={90} y={220} w={620} h={130} size={32} title="Ubuntu 24.04" sub="guest image v5 · about 350 MB" tone="guest" at="base" />
      <Chips
        x={90}
        y={386}
        w={620}
        size={23}
        at="inside"
        stagger={8}
        items={["git", "git-lfs", "gh", "Xfce desktop", "Selkies streamer", "Cua Driver", "sudo", "python3"]}
      />
      <Node x={90} y={580} w={620} h={130} title="No agent inside" sub={"Claude Code: installed on request, pinned.\nSilo never signs in for you."} at="noagent" />

      <Node id="rel" x={830} y={230} w={290} h={100} title="Release file" sub="not in the installer" at="fetch" />
      <Node id="dl" x={1190} y={230} w={290} h={100} title="Download once" sub="in the background" tone="engine" at="fetch+40" hot="fetch" />
      <Node id="sum" x={1550} y={230} w={280} h={100} title="Checksum" sub="must match the pin" tone="ok" at="fetch+100" hot="fetch" />
      <Edge from="rel" to="dl" at="fetch+44" hot="fetch" />
      <Edge from="dl" to="sum" at="fetch+104" hot="fetch" />

      <Node id="new" x={830} y={500} w={380} h={100} title="A new image version" at="pin" />
      <Node id="newc" x={1380} y={440} w={450} h={90} title="New computers use it" tone="ok" at="pin+30" hot="pin" />
      <Node id="oldc" x={1380} y={570} w={450} h={90} title="Existing ones keep theirs" at="pin+70" hot="pin" />
      <Edge from="new" to="newc" at="pin+34" hot="pin" />
      <Edge from="new" to="oldc" dashed at="pin+74" hot="pin" />
    </>
  ),
};

const create: Visual = {
  render: () => (
    <>
      <Node id="s1" x={90} y={230} w={410} h={130} title="Size" sub={"app: 8 CPUs, 32 GB, 120 GB\ncommand line: 2 CPUs, 4 GB, 10 GB"} at="config" />
      <Node id="s2" x={525} y={230} w={300} h={130} title="Image ready" sub="waits for the download" at="made" />
      <Node id="s3" x={850} y={230} w={300} h={130} title="Create, stopped" sub="by the runtime" at="made+60" hot="made" />
      <Node id="s4" x={1175} y={230} w={300} h={130} title="Verify the tools" at="verify" />
      <Node id="s5" x={1500} y={230} w={330} h={130} title="First boot" sub={"desktop +\ncomputer use"} at="verify+40" hot="verify" />
      <Edge from="s1" to="s2" at="made" />
      <Edge from="s2" to="s3" at="made+60" hot="made" />
      <Edge from="s3" to="s4" at="verify" />
      <Edge from="s4" to="s5" at="verify+40" hot="verify" />

      <Node id="stopped" x={200} y={490} w={300} h={90} title="Stopped" at="states" />
      <Node id="starting" x={620} y={490} w={300} h={90} title="Starting" tone="engine" at="states+12" hot="states" />
      <Node id="running" x={1040} y={490} w={300} h={90} title="Running" tone="ok" at="states+24" hot="states" />
      <Node id="failed" x={1460} y={490} w={300} h={90} title="Failed" tone="bad" at="states+36" hot="states" />
      <Edge from="stopped" to="starting" at="states+16" hot="states" />
      <Edge from="starting" to="running" at="states+28" hot="states" />
      <Edge from="running.b" to="stopped.b" label="stop" at="states+44" hot="states" />

      <Node id="boot" x={200} y={740} w={720} h={96} title="Every boot: check the working account" tone="engine" at="boot" />
      <Node id="halt" x={1100} y={740} w={660} h={96} title="If that fails, the computer is stopped" tone="bad" at="boot+70" hot="boot" />
      <Edge from="boot" to="halt" at="boot+70" hot="boot" />
    </>
  ),
};

const inside: Visual = {
  render: () => (
    <>
      <Group x={860} y={210} w={970} h={680} label="The computer" tone="guest" solid />
      <Node x={910} y={280} w={420} h={110} title="user: silo" sub="sudo, no password: root in effect" icon={Bot} tone="guest" at="user" />
      <Node x={1360} y={280} w={420} h={110} mono title="/workspace" sub="projects, on the persistent disk" icon={Folder} tone="guest" at="workspace" />
      <Node id="agentd" x={910} y={470} w={420} h={110} title="Guest agent" sub="runs commands, reads and writes files" at="exec" />
      <Node x={1360} y={470} w={420} h={110} title="No SSH server" sub="nothing listens in the guest" tone="ghost" at="ssh" />

      <Node id="silod" x={300} y={470} w={300} h={110} mono size={30} title="silod" icon={Cpu} tone="engine" at="exec" hot="exec tools" />
      <Edge from="silod" to="agentd" label="exec · files" at="exec+8" flow />
      <Node id="sshd" x={300} y={680} w={360} h={110} title="SSH, served by the host" sub="bridged into the guest" at="ssh" />
      <Edge from="sshd.r" to="agentd.b" at="ssh+8" />
      <Node id="term" x={90} y={250} w={230} h={90} title="Terminal" icon={SquareTerminal} tone="device" at="tools" />
      <Node id="files" x={340} y={250} w={230} h={90} title="Files" icon={Folder} tone="device" at="tools+20" hot="tools" />
      <Node id="editor" x={590} y={250} w={230} h={90} title="Your editor" icon={AppWindow} tone="device" at="tools+40" hot="tools" />
      <Edge from="term.b" to="silod.t-60" at="tools+6" />
      <Edge from="files.b" to="silod.t" at="tools+26" hot="tools" />
      <Edge from="editor.b" to="silod.t+60" at="tools+46" hot="tools" />
      <Text x={90} y={830} w={720} size={23} soft at="tools+60">
        VS Code or Zed, opened on the computer over SSH.
      </Text>
    </>
  ),
};

const macos: Visual = {
  render: () => (
    <>
      <Node x={90} y={220} w={540} h={110} title="Virtualization framework" sub="Apple silicon only" icon={Laptop} tone="device" at="vz" />
      <Node x={90} y={376} w={540} h={96} title="First: download macOS" sub="about 20 GB · about 15 minutes" at="first" />
      <Node x={90} y={500} w={540} h={96} title="Later: clone a template" sub="about a minute" tone="ok" at="first+120" hot="first" />

      <Group x={740} y={220} w={1090} h={400} label="A macOS computer" tone="guest" solid at="vz" />
      <Chips x={780} y={270} w={1010} size={24} at="setup" stagger={24} items={["account: silo", "Remote Login", "developer tools + Claude Code", "Cua Driver"]} />
      <Node x={780} y={360} w={490} h={96} title="System Integrity Protection: off" sub="set from Recovery, automatically" tone="bad" at="sip" />
      <Node x={1300} y={360} w={490} h={96} title="Screen: a native window" sub="from another Mac: screen sharing" icon={Monitor} at="window" />
      <Node x={780} y={490} w={490} h={90} title="At most two running" sub="Apple's limit" at="two" />
      <Node x={1300} y={490} w={490} h={90} title="Ports 22 and 5900" sub="always published on Mac loopback" at="window+150" hot="window" />

      <Node id="nat" x={740} y={720} w={560} h={110} title="During setup, and in Recovery" sub="Apple's own network, no Silo policy" tone="bad" at="gap" />
      <Node id="lan" x={1470} y={720} w={360} h={110} title="Your Mac, your LAN" tone="device" at="gap+60" hot="gap" />
      <Edge from="nat" to="lan" tone="bad" label="reachable" flow at="gap+66" hot="gap" />
      <Text x={90} y={740} w={560} size={25} soft at="thin">
        {"In the app: a thinner interface\nthan Linux computers, for now."}
      </Text>
    </>
  ),
};

const containers: Visual = {
  render: () => (
    <>
      <Group x={90} y={220} w={560} h={430} label="Virtual machine" tone="guest" at="third" />
      <Node x={130} y={280} w={480} h={84} title="Agent" icon={Bot} tone="guest" at="third" hot="*" />
      <Node x={130} y={400} w={480} h={84} title="Its own kernel" icon={Cpu} tone="guest" at="third" hot="*" />
      <Node x={130} y={520} w={480} h={84} title="Hypervisor" tone="device" at="third" hot="*" />

      <Group x={720} y={220} w={560} h={430} label="Container computer" tone="bad" at="third" />
      <Node x={760} y={280} w={480} h={84} title="Agent" icon={Bot} tone="guest" at="third" />
      <Node x={760} y={400} w={480} h={84} title="Rootless container" sub="crun" at="kernel" />
      <Node x={760} y={520} w={480} h={84} title="The host's kernel, shared" icon={Cpu} tone="bad" at="kernel" />
      <Text x={90} y={690} w={1190} size={27} tone="bad" at="kernel+60">
        Linux hosts only · opt-in · never the default · “less isolated than a VM”
      </Text>

      <List
        x={1350}
        y={236}
        w={490}
        gap={15}
        items={[
          { at: "hardened", text: "No capabilities" },
          { at: "hardened+18", hot: "hardened", text: "System call filter" },
          { at: "hardened+36", hot: "hardened", text: "No new privileges" },
          { at: "hardened+54", hot: "hardened", text: "No host folders" },
          { at: "same", text: "Same image, same features", tone: "ok" },
          { at: "same+50", hot: "same", text: "No memory in checkpoints", tone: "bad" },
          { at: "same+140", hot: "same", text: "Disk size not enforced", tone: "bad" },
          { at: "net", text: "One way out: the gateway", tone: "engine" },
        ]}
      />
    </>
  ),
};

const desktop: Visual = {
  render: () => (
    <>
      <Group x={90} y={240} w={720} h={330} label="The computer" tone="guest" solid />
      <Node id="xfce" x={130} y={310} w={300} h={110} title="Xfce desktop" sub="on by default" icon={Monitor} tone="guest" at="xfce" />
      <Node id="selkies" x={470} y={310} w={300} h={110} title="Selkies" sub="encodes it as video" tone="guest" at="stream" />
      <Edge from="xfce" to="selkies" at="stream+6" />
      <Chip x={410} y={474} text="listens only inside the guest" at="stream+60" hot="stream" />
      <Node id="tunnel" x={900} y={310} w={250} h={110} title="Tunnel" sub="from the engine" at="tunnel" />
      <Node id="proxy" x={1220} y={310} w={250} h={110} title="Private proxy" sub="in the engine" tone="engine" at="tunnel+45" hot="tunnel" />
      <Edge from="selkies" to="tunnel" at="tunnel+6" flow />
      <Edge from="tunnel" to="proxy" at="tunnel+50" hot="tunnel" flow />
      <Node id="v1" x={1540} y={310} w={290} h={110} title="Viewer window" sub="in the app" icon={AppWindow} tone="device" at="viewer" />
      <Node id="v2" x={1540} y={460} w={290} h={90} title="Another viewer" tone="device" at="viewer+70" hot="viewer" />
      <Edge from="proxy" to="v1" at="viewer+6" flow />
      <Edge from="proxy.r+20" to="v2.l" at="viewer+76" hot="viewer" />

      <Node id="mac" x={420} y={700} w={300} h={100} title="Your Mac" tone="device" at="clipboard" />
      <Node id="guest" x={1200} y={700} w={300} h={100} title="The computer" tone="guest" at="clipboard" />
      <Edge from="mac.r-18" to="guest.l-18" label="Paste into computer" at="clipboard+40" hot="clipboard" labelShift={[0, -26]} />
      <Edge from="guest.l+18" to="mac.r+18" label="Copy from computer" at="clipboard+80" hot="clipboard" labelShift={[0, 26]} />
      <Text x={420} y={826} w={1080} size={24} soft align="center" at="clipboard">
        Explicit actions. The clipboard is never synced.
      </Text>
    </>
  ),
};

const computerUse: Visual = {
  render: () => (
    <>
      <Group x={90} y={220} w={1040} h={640} label="The computer" tone="guest" solid />
      <Node id="desk" x={150} y={290} w={340} h={110} title="Desktop" icon={Monitor} tone="guest" at="cua" />
      <Node id="cua" x={150} y={520} w={340} h={130} title="Cua Driver" sub={"an MCP server:\nsee the screen · click · type"} icon={MousePointerClick} at="cua" hot="cua register outside" />
      <Edge from="cua" to="desk" label="drives" at="cua+10" />
      <Node id="claude" x={730} y={270} w={340} h={90} title="Claude Code" icon={Bot} tone="guest" at="register" />
      <Node id="codex" x={730} y={384} w={340} h={90} title="Codex" icon={Bot} tone="guest" at="register+40" hot="register" />
      <Edge from="claude.l" to="cua.t+60" at="register+8" />
      <Edge from="codex.l" to="cua.t+110" at="register+48" hot="register" />
      <Text x={730} y={488} w={340} size={22} soft align="center" at="register+70">
        registered after every boot
      </Text>
      <Chips x={150} y={710} w={520} size={23} at="mode" items={["ask first (default)", "automatic"]} stagger={30} />
      <Text x={150} y={776} w={900} size={23} tone="bad" at="mode+110">
        a convenience, not a security boundary: the agent has root
      </Text>

      <Node id="outer" x={1310} y={290} w={480} h={110} title="An agent on your Mac" icon={Bot} tone="device" at="outside" />
      <Node id="mcp" x={1310} y={520} w={480} h={130} mono title="silo mcp" sub="computer_use_click, …_type, …_screenshot" tone="engine" at="outside+40" hot="outside" />
      <Edge from="outer" to="mcp" at="outside+46" hot="outside" />
      <Edge from="mcp.l" to="cua.r" label="forks only" at="outside+90" hot="outside" flow labelAt={0.3} />
    </>
  ),
};

const checkpoints: Visual = {
  render: () => (
    <>
      <Node id="c1" x={110} y={380} w={280} h={90} title="Checkpoint" sub="layer" at="chain" />
      <Node id="c2" x={480} y={380} w={280} h={90} title="Checkpoint" sub="layer" at="chain+14" hot="chain restore" />
      <Node id="c3" x={850} y={380} w={320} h={90} title="Checkpoint" sub="memory + both disks" at="save+50" hot="save fork" />
      <Node id="now" x={1320} y={368} w={400} h={114} title="The computer" sub="running" icon={Bot} tone="guest" at="save" hot="save restore" />
      <Edge from="c1" to="c2" at="chain+18" />
      <Edge from="c2" to="c3" at="chain+22" />
      <Edge from="c3" to="now" at="save+56" hot="save" />
      <Chip x={110} y={496} text="each layer stores only what changed" at="chain+60" hot="chain" />
      <Node id="rec" x={1320} y={216} w={400} h={84} title="“Before restore”" sub="recovery checkpoint" tone="ok" at="restore" />
      <Edge from="now.t" to="rec.b" at="restore+6" />
      <Edge from="now.t-110" to="c2.t" label="restore" tone="engine" at="restore+70" hot="restore" />

      <Node id="fork" x={850} y={650} w={380} h={110} title="Fork: a new computer" sub="boots cold, from the disks" icon={Bot} tone="guest" at="fork" />
      <Edge from="c3.b-60" to="fork.t-60" label="fork" at="fork+8" labelShift={[40, 0]} />
      <List
        x={1320}
        y={610}
        w={520}
        gap={11}
        size={25}
        items={[
          { at: "quiet", text: "New identity", tone: "ok" },
          { at: "quiet+30", hot: "quiet", text: "Scheduled jobs disabled", tone: "ok" },
          { at: "quiet+80", hot: "quiet", text: "No secrets, unless named", tone: "ok" },
          { at: "quiet+110", hot: "quiet", text: "No GitHub, unless named: then read-only", tone: "ok" },
        ]}
      />
      <Node x={110} y={650} w={640} h={110} title="Files on disk are copied as they are" sub="a login saved in a file travels with the fork" tone="bad" at="files" />
    </>
  ),
};

const exportScene: Visual = {
  render: () => (
    <>
      <Node id="src" x={90} y={226} w={420} h={100} title="A computer or a checkpoint" tone="guest" at="file" />
      <Node id="file" x={650} y={226} w={340} h={100} mono title=".silo-backup" sub="one file" at="file+30" hot="file never disk" />
      <Node id="dst" x={1130} y={226} w={460} h={100} title="A new computer" sub="on this device or another" tone="guest" at="file+80" hot="file import" />
      <Edge from="src" to="file" label="export" at="file+34" hot="file" labelShift={[0, -24]} />
      <Edge from="file" to="dst" label="import" at="file+84" hot="file import" labelShift={[0, -24]} />
      <Chips x={1130} y={344} w={700} size={22} at="import" items={["stopped", "no secrets", "network closed"]} stagger={14} />

      <Text x={90} y={410} size={24} tone="bad" weight={600} at="never">NEVER IN THE FILE</Text>
      <List
        x={90}
        y={456}
        w={520}
        gap={10}
        size={26}
        items={[
          { at: "never", text: "Memory", tone: "bad" },
          { at: "never+30", hot: "never", text: "Secret values", tone: "bad" },
          { at: "never+55", hot: "never", text: "Published ports", tone: "bad" },
        ]}
      />
      <Text x={650} y={410} size={24} weight={600} at="disk">IN THE FILE</Text>
      <List
        x={650}
        y={456}
        w={700}
        size={26}
        items={[{ at: "disk", text: "The whole disk, unfiltered", sub: "Silo scans for known logins and asks before you share" }]}
      />
      <Text x={1130} y={410} w={700} size={24} soft at="import+110">
        Import only files you trust.
      </Text>

      <Node id="sparse" x={90} y={720} w={480} h={110} title="Sparse disks" sub="take only what the guest has used" icon={HardDrive} at="sparse" />
      <Node id="del" x={700} y={720} w={400} h={110} title="The guest deletes files" tone="guest" at="trim" />
      <Node id="back" x={1300} y={720} w={530} h={110} title="The space returns to your Mac" sub="on start, in the background, on request" tone="device" at="trim+50" hot="trim" />
      <Edge from="del" to="back" label="trim" at="trim+56" hot="trim" />
    </>
  ),
};

export const computers = {
  "linux-stack": linuxStack,
  image,
  create,
  inside,
  macos,
  containers,
  desktop,
  "computer-use": computerUse,
  checkpoints,
  export: exportScene,
};
