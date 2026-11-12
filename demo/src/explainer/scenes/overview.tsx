import React from "react";
import {
  Bot,
  Cpu,
  FileText,
  Folder,
  Globe,
  HardDrive,
  KeyRound,
  Laptop,
  MessageSquare,
  Monitor,
  Network,
  Package,
  Server,
  SquareTerminal,
  Workflow,
} from "lucide-react";
import { Chip, Edge, Group, List, Node, Text } from "../kit.tsx";
import type { Visual } from "./index.ts";

const problem: Visual = {
  render: () => (
    <>
      <Node id="issue" x={90} y={230} w={340} h={74} title="Issue" icon={FileText} at="reads" />
      <Node id="pr" x={90} y={324} w={340} h={74} title="Pull request comment" icon={MessageSquare} at="reads+5" hot="reads inject" />
      <Node id="web" x={90} y={418} w={340} h={74} title="Web page" icon={Globe} at="reads+10" />
      <Node id="dep" x={90} y={512} w={340} h={74} title="Dependency" icon={Package} at="reads+15" />
      <Node id="agent" x={660} y={350} w={300} h={130} title="Coding agent" sub="reads all of it" icon={Bot} tone="guest" at="reads" hot="reads inject reach token" />
      <Edge from="issue" to="agent.l-30" at="reads+8" />
      <Edge from="pr" to="agent.l-10" at="reads+12" />
      <Edge from="web" to="agent.l+10" at="reads+16" />
      <Edge from="dep" to="agent.l+30" at="reads+20" />
      <Edge from="pr" to="agent.l-10" at="inject" tone="bad" flow label="hidden instructions" labelShift={[0, -26]} />

      <Group x={600} y={200} w={1230} h={440} label="Your machine" tone="device" at="reach" />
      <Node id="files" x={1400} y={250} w={370} h={84} title="Your files" icon={Folder} tone="device" at="reach" hot="reach sandbox" />
      <Node id="net" x={1400} y={372} w={370} h={84} title="Your network" icon={Network} tone="device" at="reach+5" hot="reach sandbox" />
      <Node id="tokens" x={1400} y={494} w={370} h={84} title="Your tokens" icon={KeyRound} tone="device" at="reach+10" hot="reach token" />
      <Edge from="agent.r-20" to="files" at="reach+6" hot="reach sandbox" />
      <Edge from="agent.r" to="net" at="reach+10" hot="reach sandbox" />
      <Edge from="agent.r+20" to="tokens" at="reach+14" hot="reach" />
      <Chip x={1130} y={262} text="limited by the sandbox" tone="ok" at="sandbox" />
      <Chip x={1130} y={384} text="limited by the sandbox" tone="ok" at="sandbox+6" />
      <Edge from="agent.r+20" to="tokens" at="token" tone="bad" flow />
      <Chip x={1090} y={540} text="not narrowed: full authority" tone="bad" at="token" />
      <Text x={90} y={720} w={1740} size={38} weight={600} at="token+20">
        A sandbox limits where the agent can go. It does not limit what its credentials may do.
      </Text>
    </>
  ),
};

const answer: Visual = {
  render: () => (
    <>
      <Group x={90} y={210} w={1010} h={650} label="Your Mac" tone="device" />
      <Group x={480} y={300} w={560} h={490} label="A computer (virtual machine)" tone="guest" solid at="vm" />
      <Node x={530} y={360} w={460} h={96} title="Agent" icon={Bot} tone="guest" at="vm+6" hot="vm hostile" />
      <Node x={530} y={486} w={220} h={78} title="Own kernel" icon={Cpu} at="vm+12" />
      <Node x={770} y={486} w={220} h={78} title="Own disk" icon={HardDrive} at="vm+16" />
      <Node x={530} y={600} w={220} h={110} title="Repositories" sub="picked by you" at="pick" />
      <Node x={770} y={600} w={220} h={110} title="Credentials" sub="picked by you" at="pick+6" />
      <Chip x={615} y={722} text="assumed hostile, with root" tone="bad" size={22} at="hostile" />
      <Node id="engine" x={140} y={470} w={270} h={120} title="Silo engine" sub="sets the limits" tone="engine" at="two" hot="two local" />
      <Edge from="engine" to={{ x: 480, y: 530, side: "l" }} tone="engine" at="two+8" hot="two" />
      <List
        x={1170}
        y={250}
        w={670}
        gap={26}
        size={30}
        items={[
          { at: "two", text: "Reach", sub: "what the agent can connect to", tone: "engine" },
          { at: "two+10", text: "Authority", sub: "what its credentials are allowed to do", tone: "engine" },
          { at: "extras", text: "A desktop the agent can drive", icon: Monitor },
          { at: "extras+10", text: "Checkpoints to go back to", icon: HardDrive },
          { at: "local", text: "Runs locally", sub: "no Silo account, no Silo server", icon: Laptop },
        ]}
      />
    </>
  ),
};

const words: Visual = {
  render: () => (
    <>
      <Node x={90} y={210} w={560} h={170} align="left" size={32} title="device" sub={"A physical machine that runs Silo:\nyour Mac, or a Linux server"} icon={Laptop} tone="device" at="device" />
      <Node x={680} y={210} w={560} h={170} align="left" size={32} title="computer" sub={"A virtual one, which Silo creates\nfor an agent"} icon={Bot} tone="guest" at="computer" />
      <Node x={1270} y={210} w={560} h={170} align="left" size={32} title="workspace" sub={"/workspace inside a computer,\non its own disk"} icon={Folder} tone="guest" at="workspace" />
      <Node x={90} y={410} w={855} h={170} align="left" size={32} title="checkpoint · fork" sub={"A saved state of a computer ·\na new computer made from one"} icon={HardDrive} at="checkpoint" />
      <Node x={975} y={410} w={855} h={170} align="left" size={32} title="Connections" sub={"Links devices, so one Silo manages\ncomputers on another"} icon={Network} tone="device" at="connections" />
      <Node x={90} y={690} w={560} h={130} size={30} title="Your device" sub="trusted" tone="device" at="colours+25" hot="colours" />
      <Node x={680} y={690} w={560} h={130} size={30} title="Silo's engine" sub="in between" tone="engine" at="colours+175" hot="colours" />
      <Node x={1270} y={690} w={560} h={130} size={30} title="A computer and its agent" sub="not trusted" tone="guest" at="colours+95" hot="colours" />
    </>
  ),
};

const map: Visual = {
  render: () => (
    <>
      <Node id="app" x={90} y={200} w={320} h={92} title="Silo app" icon={Laptop} tone="device" at="clients" />
      <Node id="cli" x={445} y={200} w={320} h={92} title="silo CLI" icon={SquareTerminal} tone="device" at="clients+30" />
      <Node id="mcp" x={800} y={200} w={320} h={92} title="MCP server" icon={Bot} tone="device" at="clients+75" />
      <Node id="runner" x={1155} y={200} w={320} h={92} title="CI runner" icon={Workflow} tone="device" at="clients+140" />
      <Node id="embed" x={1510} y={200} w={320} h={92} title="Apps that embed it" sub="such as Fjord" tone="device" at="clients+190" />
      <Node id="sock" x={640} y={362} w={640} h={66} mono title="engine.sock · one protocol" at="socket" />
      <Edge from="app.b" to="sock.t-240" at="socket+4" />
      <Edge from="cli.b" to="sock.t-120" at="socket+7" />
      <Edge from="mcp.b" to="sock.t" at="socket+10" />
      <Edge from="runner.b" to="sock.t+120" at="socket+13" />
      <Edge from="embed.b" to="sock.t+240" at="socket+16" />
      <Node id="silod" x={560} y={496} w={800} h={112} size={34} mono title="silod" sub="the engine: computers · credentials · network rules" icon={Cpu} tone="engine" at="engine" />
      <Edge from="sock" to="silod" tone="engine" at="engine+6" flow />
      <Node id="msb" x={330} y={676} w={480} h={84} title="MicroSandbox" sub="Linux computers" at="runtimes" />
      <Node id="vz" x={1110} y={676} w={480} h={84} title="Virtualization framework" sub="macOS computers" at="runtimes+60" />
      <Edge from="silod.b-200" to="msb.t" at="runtimes+6" />
      <Edge from="silod.b+200" to="vz.t" at="runtimes+66" />
      <Node id="l1" x={250} y={826} w={300} h={80} title="Linux computer" icon={Bot} tone="guest" at="guests" />
      <Node id="l2" x={590} y={826} w={300} h={80} title="Linux computer" icon={Bot} tone="guest" at="guests+5" />
      <Node id="m1" x={1200} y={826} w={300} h={80} title="macOS computer" icon={Bot} tone="guest" at="guests+10" />
      <Edge from="msb.b-90" to="l1.t" at="guests+8" />
      <Edge from="msb.b+90" to="l2.t" at="guests+12" />
      <Edge from="vz.b" to="m1.t" at="guests+16" />
    </>
  ),
};

const status: Visual = {
  render: () => (
    <>
      <Node x={90} y={230} w={850} h={190} size={36} title="MIT · one person · work in progress" sub="open source, local, no account" at="who" />
      <Node x={980} y={230} w={850} h={190} size={36} title="app 0.13 · engine 0.2 · protocol 7" sub="the app and the engine are released separately" tone="engine" at="versions" />
      <Node x={90} y={470} w={850} h={190} size={36} title="Host: Apple silicon Mac" sub="the engine alone also runs on Linux servers" icon={Laptop} tone="device" at="host" />
      <Node x={980} y={470} w={850} h={190} size={36} title="Guests: Ubuntu 24.04 · macOS" sub="each one a computer for an agent" icon={Server} tone="guest" at="guests" />
    </>
  ),
};

export const overview = { problem, answer, words, map, status };
