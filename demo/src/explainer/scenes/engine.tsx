import React from "react";
import { Bot, Cpu, Database, Laptop, Network, SquareTerminal } from "lucide-react";
import { Chip, Chips, Code, Edge, Group, Node, Text } from "../kit.tsx";
import type { Visual } from "./index.ts";

const processes: Visual = {
  render: () => (
    <>
      <Group x={90} y={200} w={1740} h={700} label="Your Mac" tone="device" />
      <Node id="launchd" x={150} y={270} w={280} h={84} mono title="launchd" sub="at login" at="launchd" />
      <Group x={610} y={390} w={520} h={330} label="one process" tone="engine" at="vz" />
      <Node id="silod" x={660} y={430} w={420} h={116} size={32} mono title="silod" sub="the engine, one per user account" icon={Cpu} tone="engine" at="launchd+10" hot="launchd quit" />
      <Edge from="launchd" to="silod.t-100" label="starts" at="launchd+16" />
      <Node id="app" x={150} y={438} w={300} h={100} title="Silo app" sub="a client only" icon={Laptop} tone="device" at="app" hot="app quit" />
      <Edge from="app" to="silod" label="socket" at="app+8" flow />
      <Node id="msb1" x={1270} y={270} w={230} h={84} mono title="msb" sub="one process" at="msb" />
      <Node id="msb2" x={1270} y={384} w={230} h={84} mono title="msb" sub="one process" at="msb+6" />
      <Node id="c1" x={1560} y={270} w={230} h={84} title="Linux" sub="computer" tone="guest" at="msb+10" />
      <Node id="c2" x={1560} y={384} w={230} h={84} title="Linux" sub="computer" tone="guest" at="msb+14" />
      <Edge from="silod.r-30" to="msb1.l" at="msb+4" />
      <Edge from="silod.r-10" to="msb2.l" at="msb+8" />
      <Edge from="msb1" to="c1" at="msb+12" />
      <Edge from="msb2" to="c2" at="msb+16" />
      <Node id="mac" x={700} y={590} w={340} h={90} title="macOS computer" icon={Bot} tone="guest" at="vz" />
      <Node id="gw" x={1270} y={593} w={330} h={84} mono title="gateway" sub="its only network" icon={Network} at="gateway" />
      <Edge from="mac" to="gw" at="gateway+8" />
      <Text x={150} y={760} size={26} soft at="quit">
        Quit asks:
      </Text>
      <Chips x={300} y={752} w={900} size={24} items={["Cancel", "Keep running", "Stop and quit"]} at="quit+40" hot="quit" stagger={22} />
      <Text x={150} y={826} w={1500} size={24} soft at="quit">
        Quitting, updating or crashing the app leaves the engine and its computers running.
      </Text>
    </>
  ),
};

const wire: Visual = {
  render: () => (
    <>
      <Node id="client" x={120} y={220} w={300} h={110} title="A client" sub="app, CLI, MCP, runner" icon={Laptop} tone="device" at="file" hot="file hello request" />
      <Node id="sock" x={720} y={234} w={480} h={82} mono title="~/.silo/engine.sock" at="file" hot="file uid" />
      <Node id="silod" x={1500} y={220} w={300} h={110} mono size={30} title="silod" icon={Cpu} tone="engine" at="file" hot="file events hello" />
      <Edge from="client" to="sock" at="file+8" both />
      <Edge from="sock" to="silod" at="file+12" both />
      <Chips x={760} y={334} w={460} items={["mode 0600", "same user only"]} at="uid" />
      <Node x={470} y={440} w={250} h={90} mono title="length" sub="4 bytes, big-endian" tone="engine" at="frame" />
      <Node x={720} y={440} w={730} h={90} mono title="{ JSON }" sub="one message, up to 4 MiB" at="frame+6" hot="frame" />
      <Code
        x={300}
        y={590}
        w={1320}
        h={300}
        size={24}
        title="one connection"
        at="hello"
        lines={[
          { at: "hello", text: '→  engine.handshake   { protocol: { min: 7, max: 7 } }', tone: "device" },
          { at: "hello+70", text: '←  { protocol: 7, deviceId: "…", capabilities: […] }', tone: "engine" },
          { at: "request", text: '→  { id: "12", method: "computers.list", params: {} }', tone: "device" },
          { at: "request+80", text: '←  { id: "12", result: [ … ] }', tone: "engine" },
          { at: "events", text: '←  { event: "computers.changed", seq: 41, payload: … }', tone: "engine" },
          { at: "events+90", text: '←  { event: "operations.job.progress", seq: 42, … }', tone: "engine" },
        ]}
      />
    </>
  ),
};

const methods: Visual = {
  render: () => (
    <>
      <Text x={90} y={186} size={150} weight={700} tone="engine" at="count">
        175
      </Text>
      <Text x={400} y={262} w={900} size={34} soft at="count">
        typed methods: parameters in, result out
      </Text>
      <Node x={90} y={400} w={330} h={130} size={30} title="54 queries" sub="read state" at="query" />
      <Node x={440} y={400} w={330} h={130} size={30} title="73 commands" sub="change it, answer at once" at="query+45" hot="query" />
      <Node x={790} y={400} w={330} h={130} size={30} title="42 jobs" sub="slow work, returns a job ID" tone="engine" at="job" />
      <Node x={1140} y={400} w={330} h={130} size={30} title="4 streams" sub="raw bytes: SSH, screens" at="stream" />
      <Node x={1490} y={400} w={340} h={130} size={30} title="2 subscriptions" sub="events, file changes" at="stream+95" hot="stream" />
      <Chips
        x={90}
        y={580}
        w={1740}
        mono
        size={22}
        at="groups"
        stagger={3}
        items={[
          "computers", "guest", "changes", "github", "connections", "exports", "display", "engine", "macos",
          "checkpoints", "updates", "network", "migration", "secrets", "push", "operations", "sshAccess", "settings",
          "recipes", "guestClient", "files", "desktop", "computerUse", "clipboard", "storage", "steps", "preparation",
          "logs", "events", "editor", "device", "terminal", "notifications", "activity",
        ]}
      />
      <Node id="api" x={250} y={780} w={380} h={92} mono title="silo-api" sub="Rust types" at="schema" />
      <Node id="schema" x={770} y={780} w={460} h={92} mono title="silo-api.schema.json" sub="generated" at="schema+10" />
      <Node id="any" x={1370} y={780} w={380} h={92} title="A client in any language" at="schema+20" />
      <Edge from="api" to="schema" at="schema+14" />
      <Edge from="schema" to="any" at="schema+24" />
    </>
  ),
};

const jobs: Visual = {
  render: () => (
    <>
      <Node id="call" x={90} y={220} w={360} h={90} mono title="computers.create" tone="device" at="progress" />
      <Node id="job" x={560} y={220} w={240} h={90} mono title="job:42" tone="engine" at="progress+6" hot="progress stored" />
      <Node id="p1" x={900} y={230} w={190} h={70} title="progress" at="progress+20" />
      <Node id="p2" x={1120} y={230} w={190} h={70} title="progress" at="progress+34" />
      <Node id="done" x={1340} y={230} w={230} h={70} title="finished" tone="ok" at="progress+52" hot="progress ack" />
      <Edge from="call" to="job" at="progress+10" />
      <Edge from="job" to="p1" at="progress+24" />
      <Edge from="p1" to="p2" at="progress+38" />
      <Edge from="p2" to="done" at="progress+56" />
      <Node id="db" x={470} y={390} w={420} h={96} mono title="registry.sqlite3" sub="jobs outlive a restart" icon={Database} at="stored" />
      <Edge from="job" to="db" dashed at="stored+8" />
      <Text x={940} y={398} w={880} size={24} soft at="stored+110">
        {"After a restart, an unfinished job answers:\n“The engine restarted before this job completed.”"}
      </Text>
      <Chip x={1600} y={246} text="acknowledged" tone="ok" at="ack" />

      <Group x={90} y={560} w={1740} h={340} label="The queue" tone="engine" at="lanes" />
      <Text x={140} y={618} size={25} soft at="lanes">computer A</Text>
      <Text x={140} y={708} size={25} soft at="lanes">computer B</Text>
      <Text x={140} y={798} size={25} soft at="wide">whole device</Text>
      <Node x={400} y={600} w={220} h={66} title="start" tone="guest" at="lanes+6" hot="lanes" />
      <Node x={650} y={600} w={260} h={66} title="checkpoint" tone="guest" at="lanes+12" hot="lanes" />
      <Node x={400} y={690} w={220} h={66} title="fork" tone="guest" at="lanes+18" hot="lanes" />
      <Node x={650} y={690} w={260} h={66} title="export" tone="guest" at="lanes+24" hot="lanes" />
      <Text x={960} y={650} w={420} size={24} soft at="lanes+30">lanes run in parallel</Text>
      <Node x={1380} y={780} w={400} h={66} title="waits for everything" tone="engine" at="wide" />
    </>
  ),
};

const crates: Visual = {
  render: () => (
    <>
      <Node id="api" x={740} y={200} w={440} h={88} mono title="silo-api" sub="the contract" at="api" />
      <Node id="client" x={740} y={340} w={440} h={88} mono title="silo-client" sub="framing · handshake · events" at="client" />
      <Edge from="client" to="api" at="client+8" />
      <Node id="engine" x={1340} y={640} w={440} h={96} mono title="silo-engine" sub="the engine, as a library" tone="engine" at="engine" />
      <Node id="silod" x={1340} y={440} w={440} h={96} mono title="silod" sub="socket · dispatcher · timers" icon={Cpu} tone="engine" at="silod" />
      <Edge from="silod" to="engine" at="silod+8" hot="silod context" />
      <Edge from="silod.l-20" to="api.r" at="silod+12" />
      <Node id="ui" x={140} y={440} w={440} h={96} mono title="silo-ui" sub="the app: Tauri shell + React" icon={Laptop} tone="device" at="ui" />
      <Node id="cli" x={140} y={640} w={440} h={96} mono title="silo-cli" sub="command line + MCP server" icon={SquareTerminal} tone="device" at="cli" />
      <Edge from="ui.r-20" to="client.l" at="ui+8" />
      <Edge from="cli.r-20" to="client.l+20" at="cli+8" />
      <Chips
        x={140}
        y={810}
        w={1100}
        mono
        size={23}
        at="rest"
        stagger={16}
        items={["silo-host · shared helpers", "macos-gateway · network", "container-shim", "runner · Go"]}
      />
      <Chip x={1150} y={566} text="EngineContext: paths · identity · events · main thread" tone="engine" mono size={20} at="context" />
    </>
  ),
};

const disk: Visual = {
  render: () => (
    <>
      <Code
        x={90}
        y={220}
        w={540}
        h={346}
        title="~/.silo"
        at="home"
        lines={[
          { text: "engine.sock", tone: "engine" },
          { text: "engine.lock" },
          { text: "engine/registry.sqlite3" },
          { text: "desktop-remote/", soft: true },
        ]}
      />
      <Code
        x={690}
        y={220}
        w={540}
        h={346}
        title="Application Support"
        at="appdata"
        lines={[
          { text: "settings.json" },
          { text: "github.json   secrets.json" },
          { text: "  names and references only", soft: true },
          { text: "runtime/      computers, disks", tone: "guest" },
          { text: "guest-image/" },
          { at: "keys", text: "runtime/…/ssh/        key files", tone: "bad" },
          { at: "keys+60", text: "runtime/…/tls/ca.key  key file", tone: "bad" },
        ]}
      />
      <Code
        x={1290}
        y={220}
        w={540}
        h={346}
        title="Keychain"
        at="keychain"
        lines={[
          { text: "org.silo.Silo.github", tone: "device" },
          { text: "org.silo.Silo.secrets", tone: "device" },
          { text: "org.silo.Silo.proxy-ca", tone: "device" },
          { text: "the values themselves", soft: true },
        ]}
      />
      <Node id="prod" x={250} y={690} w={560} h={140} size={30} title="Silo" sub={"production · org.silo.preview\n~/.silo"} tone="device" at="channels" />
      <Node id="dev" x={1110} y={690} w={560} h={140} size={30} title="Silo Dev" sub={"development · org.silo.dev\n~/.silo-dev"} at="channels+10" hot="channels" />
      <Edge from="prod" to="dev" blocked label="never share state" at="channels+40" hot="channels" />
    </>
  ),
};

const lifecycle: Visual = {
  render: () => (
    <>
      <Text x={90} y={196} size={22} soft>START</Text>
      <Node id="a" x={90} y={236} w={400} h={104} title="Check its own signature" sub="production refuses without one" at="sign" />
      <Node id="b" x={535} y={236} w={400} h={104} title="Take the lock" sub="only one engine runs" at="lock" />
      <Node id="c" x={980} y={236} w={400} h={104} title="Convert saved data" sub="from older versions" at="open" />
      <Node id="d" x={1425} y={236} w={405} h={104} title="Open the socket" tone="engine" at="open+45" hot="open" />
      <Edge from="a" to="b" at="lock" />
      <Edge from="b" to="c" at="open" />
      <Edge from="c" to="d" at="open+45" hot="open" />
      <Text x={1130} y={352} w={700} size={22} soft align="right" at="open+110">
        then, in the background: resume computers, start those set to start at launch
      </Text>

      <Text x={90} y={426} size={22} soft at="update">UPDATE</Text>
      <Node id="u1" x={90} y={466} w={330} h={104} title="App update" sub="downloaded, verified" tone="device" at="update" />
      <Node id="u2" x={465} y={466} w={430} h={104} title="Save running computers" sub="behind a crash-safe journal" at="update+70" hot="update" />
      <Node id="u3" x={940} y={466} w={300} h={104} title="Engine exits" at="update+120" hot="update" />
      <Node id="u4" x={1285} y={466} w={545} h={104} title="New engine resumes that set" tone="engine" at="update+160" hot="update" />
      <Edge from="u1" to="u2" at="update+70" hot="update" />
      <Edge from="u2" to="u3" at="update+120" hot="update" />
      <Edge from="u3" to="u4" at="update+160" hot="update" />

      <Text x={90} y={656} size={22} soft at="watch">WHILE RUNNING</Text>
      <Node id="w1" x={90} y={696} w={420} h={104} title="Health watch" sub="polls every 10 seconds" tone="engine" at="watch" />
      <Node id="w2" x={700} y={696} w={640} h={104} title="Notice" sub="a change that no Silo operation caused" at="watch+60" hot="watch" />
      <Edge from="w1" to="w2" at="watch+60" hot="watch" />
    </>
  ),
};

export const engine = { processes, wire, methods, jobs, crates, disk, lifecycle };
