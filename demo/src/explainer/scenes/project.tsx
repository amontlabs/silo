import React from "react";
import { TriangleAlert } from "lucide-react";
import { Edge, Group, List, Node, Text } from "../kit.tsx";
import type { Visual } from "./index.ts";

const build: Visual = {
  render: () => (
    <>
      <Node x={90} y={230} w={560} h={130} size={30} title="Rust" sub="engine · daemon · app shell · command line" at="stack" />
      <Node x={680} y={230} w={560} h={130} size={30} title="React + TypeScript" sub="the interface" at="stack+125" hot="stack" />
      <Node x={1270} y={230} w={560} h={130} size={30} title="Go" sub="the CI runner" at="stack+190" hot="stack" />
      <Node x={90} y={450} w={560} h={110} title="Interface tests" at="ci" />
      <Node x={680} y={450} w={560} h={110} title="Rust tests, on a Mac" at="ci+55" hot="ci" />
      <Node x={1270} y={450} w={560} h={110} title="Engine tests, on Linux" at="ci+105" hot="ci" />
      <Node x={90} y={650} w={860} h={110} title="A real computer, booted on a Linux runner" sub="create · start · exec · checkpoint · fork · stop" tone="ok" at="kvm" />
      <Node id="dev" x={1060} y={650} w={300} h={110} title="Silo Dev" at="dev" />
      <Node id="prod" x={1530} y={650} w={300} h={110} title="Production data" tone="device" at="dev" />
      <Edge from="dev" to="prod" blocked at="dev+50" hot="dev" />
    </>
  ),
};

const release: Visual = {
  render: () => (
    <>
      <Node x={90} y={230} w={840} h={120} size={30} title="The app · v0.13.0" sub="a disk image that updates itself" tone="device" at="two" />
      <Node x={990} y={230} w={840} h={120} size={30} title="The engine · engine-v0.2.0" sub="an archive: silod, service files, the API schema" tone="engine" at="two+130" hot="two" />
      <Node id="other" x={990} y={430} w={400} h={110} title="An embedding app" sub="bundles and signs silod" at="embed" />
      <Node id="req" x={1430} y={430} w={400} h={110} title="A code requirement" sub="the signature the engine must carry" tone="engine" at="embed+90" hot="embed" />
      <Edge from="other" to="req" at="embed+96" hot="embed" />
      <Node x={90} y={430} w={400} h={110} title="Not notarized" sub="first launch: Open Anyway" tone="bad" at="notarized" />
      <Node x={530} y={430} w={400} h={110} title="Updates: verified" sub="signed with a separate key" tone="ok" at="notarized+160" hot="notarized" />
      <Node x={990} y={640} w={840} h={110} title="No update channel of its own yet" sub="the engine updates with the app" at="feed" />
    </>
  ),
};

const gaps: Visual = {
  render: () => (
    <>
      <List
        x={90}
        y={230}
        w={840}
        gap={30}
        size={29}
        items={[
          { at: "egress", text: "No egress allowlist", sub: "data the agent can read can leave; no traffic limit", icon: TriangleAlert, tone: "bad" },
          { at: "echo", text: "Reflected secrets not redacted", sub: "an allowed host can echo the value back", icon: TriangleAlert, tone: "bad" },
          { at: "socket", text: "No client identity, no audit log", sub: "anything running as you controls the engine", icon: TriangleAlert, tone: "bad" },
          { at: "share", text: "The /.msb shared folder", sub: "read-write to the guest, unreviewed", icon: TriangleAlert, tone: "bad" },
        ]}
      />
      <List
        x={1000}
        y={230}
        w={830}
        gap={30}
        size={29}
        items={[
          { at: "macos", text: "macOS computers", sub: "exposed during setup; not covered by the evals", icon: TriangleAlert, tone: "bad" },
          { at: "pat", text: "Personal tokens", sub: "not scoped at all", icon: TriangleAlert, tone: "bad" },
          { at: "protocol", text: "One protocol version", sub: "every change breaks older clients", icon: TriangleAlert, tone: "bad" },
          { at: "docs", text: "Documentation behind the code", sub: "the README calls the Linux engine a next milestone", icon: TriangleAlert, tone: "bad" },
        ]}
      />
    </>
  ),
};

const roadmap: Visual = {
  render: () => (
    <>
      <Group x={90} y={230} w={540} h={500} label="In progress" tone="engine" at="now" />
      <List x={130} y={290} w={460} items={[{ at: "now", text: "Cua Driver for computer use", sub: "finishing the move" }]} />
      <Group x={690} y={230} w={540} h={500} label="Next" tone="device" at="next" />
      <List
        x={730}
        y={290}
        w={460}
        gap={22}
        items={[
          { at: "next+50", hot: "next", text: "An egress policy per computer" },
          { at: "next+110", hot: "next", text: "Redacting reflected secrets" },
          { at: "next+160", hot: "next", text: "Client identity and an audit log" },
          { at: "next+215", hot: "next", text: "A review of the shared folder" },
          { at: "next+290", hot: "next", text: "Finishing the Linux host" },
          { at: "next+335", hot: "next", text: "A protocol reference" },
          { at: "next+375", hot: "next", text: "Project governance" },
        ]}
      />
      <Group x={1290} y={230} w={540} h={500} label="When funded" tone="ghost" at="then" />
      <List
        x={1330}
        y={290}
        w={460}
        gap={22}
        items={[
          { at: "then+40", hot: "then", text: "Evals in CI" },
          { at: "then+90", hot: "then", text: "An external security review" },
          { at: "then+150", hot: "then", text: "Signed and notarized releases" },
          { at: "then+220", hot: "then", text: "Published crates" },
        ]}
      />
    </>
  ),
};

const Logo: React.FC = () => (
  <svg viewBox="0 0 128 128" width={150} height={150}>
    <g fill="none" stroke="#f3eee4" strokeWidth={7} strokeLinecap="round">
      <circle cx={64} cy={64} r={39} strokeDasharray="200 46" transform="rotate(-28 64 64)" />
      <circle cx={64} cy={64} r={26} strokeDasharray="126 37" transform="rotate(63 64 64)" />
      <circle cx={64} cy={64} r={13} stroke="#ff9f0a" strokeDasharray="58 24" transform="rotate(154 64 64)" />
    </g>
  </svg>
);

const title: Visual = {
  bare: true,
  render: () => (
    <>
      <Text x={0} y={300} w={1920} align="center">
        <Logo />
      </Text>
      <Text x={0} y={480} w={1920} size={120} weight={700} align="center">
        Silo
      </Text>
      <Text x={0} y={640} w={1920} size={38} align="center" soft at="title+25">
        What it is, how it is built, and where its limits are
      </Text>
    </>
  ),
};

const recap: Visual = {
  render: () => (
    <>
      <List
        x={200}
        y={230}
        w={1520}
        gap={34}
        size={34}
        items={[
          { at: "engine", text: "A local engine. One virtual machine per agent.", tone: "engine" },
          { at: "clients", text: "One socket: an app, a command line, an MCP server, a CI runner.", tone: "device" },
          { at: "hostile", text: "A hostile agent: public addresses only, credentials it cannot read, the repositories you grant.", tone: "guest" },
          { at: "loop", text: "Fork a base computer, review what changed, keep it or drop it." },
          { at: "limits", text: "Stated limits: readable data can leave; anything running as you controls the engine.", tone: "bad" },
        ]}
      />
      <Text x={860} y={730} at="end">
        <Logo />
      </Text>
    </>
  ),
};

export const project = { title, build, release, gaps, roadmap, recap };
