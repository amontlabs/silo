import React from "react";
import { Bot, Cpu, TriangleAlert, Workflow } from "lucide-react";
import { Chips, Code, Edge, Group, Node, Text } from "../kit.tsx";
import type { Visual } from "./index.ts";

const connections: Visual = {
  render: () => (
    <>
      <Group x={90} y={220} w={640} h={530} label="Your Mac · the controller" tone="device" />
      <Group x={1080} y={220} w={750} h={530} label="The other device · the owner" tone="device" />
      <Node id="mine" x={150} y={300} w={320} h={110} mono size={30} title="silod" sub="yours" icon={Cpu} tone="engine" at="what" />
      <Node id="theirs" x={1590} y={300} w={200} h={110} mono size={30} title="silod" sub="its own" icon={Cpu} tone="engine" at="what" />
      <Node id="vms" x={1540} y={490} w={250} h={90} title="Its computers" icon={Bot} tone="guest" at="what" />
      <Edge from="theirs" to="vms" at="what+8" />
      <Node id="sshd" x={1120} y={300} w={200} h={110} mono title="sshd" sub="your key" at="ssh" />
      <Text x={1120} y={424} w={440} size={21} soft at="ssh+70">restricted to one command: the bridge</Text>
      <Edge from="mine" to="sshd" label="plain SSH" at="ssh+8" flow labelShift={[0, -26]} />
      <Node id="bridge" x={1350} y={300} w={210} h={110} mono size={22} title="silo-remote" sub="the bridge" at="bridge" />
      <Edge from="sshd" to="bridge" at="bridge+6" />
      <Edge from="bridge" to="theirs" at="bridge+12" flow />
      <Chips x={1120} y={630} w={680} size={22} at="owner" stagger={40} items={["its secrets stay here", "its GitHub sign-in stays here"]} />
      <Node x={150} y={500} w={520} h={130} title="Roughly full authority there" sub="no expiry; a revoke leaves open sessions" icon={TriangleAlert} tone="bad" at="authority" />
      <Node x={640} y={810} w={640} h={76} title="Both devices: the same bridge version" at="version" />
    </>
  ),
};

const linuxHost: Visual = {
  render: () => (
    <>
      <Group x={1010} y={220} w={820} h={540} label="A Linux server" tone="device" />
      <Node id="silod" x={1070} y={300} w={330} h={110} mono size={30} title="silod" sub="a user service, no interface" icon={Cpu} tone="engine" at="service" />
      <Node id="kvm" x={1070} y={480} w={330} h={90} title="KVM" at="service+60" hot="service" />
      <Node id="vms" x={1450} y={480} w={330} h={90} title="Linux computers" icon={Bot} tone="guest" at="service+60" hot="service" />
      <Edge from="silod" to="kvm" at="service+66" hot="service" />
      <Edge from="kvm" to="vms" at="service+70" hot="service" />
      <Code
        x={90}
        y={220}
        w={840}
        h={250}
        size={23}
        title="on the server"
        at="init"
        lines={[
          { text: "$ silo host init --authorize <your key>" },
          { at: "init+50", text: "  ✓ /dev/kvm", tone: "ok" },
          { at: "init+85", text: "  ✓ service registered", tone: "ok" },
          { at: "init+120", text: "  ✓ key authorised", tone: "ok" },
        ]}
      />
      <Code
        x={90}
        y={510}
        w={840}
        h={250}
        size={23}
        title="on your Mac"
        at="device"
        lines={[
          { text: "$ silo devices add user@server" },
          { at: "device+70", text: "$ silo --device server list" },
          { at: "device+70", text: "$ silo --device server fork main" },
        ]}
      />
      <Node x={1070} y={620} w={330} h={96} title="Secrets: files" sub="readable by that account" at="differs" />
      <Node x={1450} y={620} w={330} h={96} title="Nothing signed yet" tone="bad" at="differs+110" hot="differs" />
    </>
  ),
};

const runner: Visual = {
  render: () => (
    <>
      <Node id="gha" x={90} y={240} w={360} h={120} title="GitHub Actions" sub="runs-on: silo-linux" icon={Workflow} at="what" />
      <Node id="runner" x={600} y={240} w={340} h={120} mono size={26} title="silo-runner" sub="a Go helper, on the host" tone="engine" at="what" />
      <Edge from="gha" to="runner" both label="jobs" at="what+10" labelShift={[0, -24]} />
      <Node id="cli" x={1090} y={240} w={300} h={120} mono size={26} title="silo --json" sub="the command line" tone="device" at="cli" />
      <Node id="silod" x={1540} y={240} w={290} h={120} mono size={30} title="silod" icon={Cpu} tone="engine" at="cli+70" hot="cli" />
      <Edge from="runner" to="cli" at="cli+8" />
      <Edge from="cli" to="silod" at="cli+76" hot="cli" />
      <Node id="cp" x={600} y={500} w={340} h={96} title="Recipe checkpoint" at="fresh" />
      <Node id="j1" x={1090} y={440} w={320} h={84} title="Job 1: a computer" icon={Bot} tone="guest" at="fresh+30" hot="fresh" />
      <Node id="j2" x={1090} y={560} w={320} h={84} title="Job 2: a computer" icon={Bot} tone="guest" at="fresh+50" hot="fresh" />
      <Edge from="cp.r-14" to="j1.l" label="fork" at="fresh+34" hot="fresh" />
      <Edge from="cp.r+14" to="j2.l" at="fresh+54" hot="fresh" />
      <Node id="ok" x={1530} y={440} w={300} h={84} title="Done: deleted" tone="ok" at="end" />
      <Node id="kept" x={1530} y={560} w={300} h={84} title="Failed: kept a few days" at="end+60" hot="end" />
      <Edge from="j1" to="ok" at="end+6" />
      <Edge from="j2" to="kept" at="end+66" hot="end" />
      <Node x={90} y={690} w={840} h={96} title="Public repositories: refused" sub="checked again before every job" icon={TriangleAlert} tone="bad" at="private" />
      <Node x={990} y={690} w={840} h={96} title="A job's GitHub secrets: unprotected" sub="they enter its computer as they are" icon={TriangleAlert} tone="bad" at="secrets" />
      <Node x={90} y={812} w={840} h={84} title="Checked live on a Mac" tone="ok" at="live" />
      <Node x={990} y={812} w={840} h={84} title="A job on a Linux host: not run yet" tone="bad" at="live+70" hot="live" />
    </>
  ),
};

export const beyond = { connections, "linux-host": linuxHost, runner };
