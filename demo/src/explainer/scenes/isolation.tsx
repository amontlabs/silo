import React from "react";
import { Bot, CircleCheck, Cpu, Globe, KeyRound, Laptop, Network, TriangleAlert } from "lucide-react";
import { Chip, Chips, Edge, Grid, Group, List, Node, Text } from "../kit.tsx";
import type { Visual } from "./index.ts";

const threat: Visual = {
  render: () => (
    <>
      <Node id="agent" x={90} y={330} w={470} h={170} size={32} title="The agent" sub={"prompt injected · root in its computer\nhas read Silo's source"} icon={Bot} tone="guest" at="attacker" hot="attacker bounds" />
      <Node x={1300} y={210} w={530} h={74} align="left" title="1  Your Mac and your account" tone="device" at="assets" />
      <Node x={1300} y={300} w={530} h={74} align="left" title="2  Your GitHub authority" tone="device" at="assets+50" hot="assets" />
      <Node x={1300} y={390} w={530} h={74} align="left" title="3  Your secret values" tone="device" at="assets+85" hot="assets" />
      <Node x={1300} y={480} w={530} h={74} align="left" title="4  Your private network" tone="device" at="assets+120" hot="assets" />
      <Node x={1300} y={570} w={530} h={74} align="left" title="5  Your other computers" tone="device" at="assets+155" hot="assets" />
      <Node x={1300} y={720} w={530} h={110} align="left" title="Not protected" sub="the data inside the computer" tone="bad" at="notdata" />
      <Node id="b1" x={720} y={220} w={420} h={84} title="The virtual machine" tone="engine" at="bounds+25" hot="bounds" />
      <Node id="b2" x={720} y={332} w={420} h={84} title="The network policy" tone="engine" at="bounds+55" hot="bounds" />
      <Node id="b3" x={720} y={444} w={420} h={84} title="The secrets proxy" tone="engine" at="bounds+85" hot="bounds" />
      <Node id="b4" x={720} y={556} w={420} h={84} title="Tokens minted on the host" tone="engine" at="bounds+115" hot="bounds" />
      <Edge from="agent.r-30" to="b1.l" at="bounds+30" hot="bounds" />
      <Edge from="agent.r-10" to="b2.l" at="bounds+60" hot="bounds" />
      <Edge from="agent.r+10" to="b3.l" at="bounds+90" hot="bounds" />
      <Edge from="agent.r+30" to="b4.l" at="bounds+120" hot="bounds" />
    </>
  ),
};

const network: Visual = {
  render: () => (
    <>
      <Group x={90} y={210} w={1010} h={690} label="Your Mac" tone="device" />
      <Group x={140} y={290} w={330} h={190} label="Computer" tone="guest" solid />
      <Node id="agent" x={175} y={340} w={260} h={96} title="Agent" icon={Bot} tone="guest" at="where" />
      <Node id="stack" x={600} y={320} w={360} h={136} title="Network stack" sub={"inside the runtime process:\nthe policy is applied here"} icon={Network} tone="engine" at="where" hot="where public private dns" />
      <Edge from="agent" to="stack" at="where+8" flow="always" />
      <Node id="inet" x={1320} y={240} w={440} h={110} title="Public addresses" icon={Globe} tone="ok" at="public" hot="public open" />
      <Edge from="stack.r-30" to="inet.l" tone="ok" label="allowed" at="public+8" flow hot="public open" />
      <Node id="lan" x={1320} y={430} w={440} h={84} title="Local network · VPN" at="private+60" hot="private" />
      <Node id="meta" x={1320} y={540} w={440} h={84} title="Cloud metadata" at="private+120" hot="private" />
      <Node id="loop" x={140} y={570} w={330} h={76} title="Mac loopback" tone="device" at="private+25" hot="private" />
      <Node id="others" x={140} y={680} w={330} h={76} title="Other computers" tone="guest" at="private+150" hot="private" />
      <Edge from="stack.r+10" to="lan.l" blocked at="private+66" hot="private" />
      <Edge from="stack.r+40" to="meta.l" blocked at="private+126" hot="private" />
      <Edge from="stack.b-110" to="loop.r" blocked at="private+31" hot="private" />
      <Edge from="stack.b-60" to="others.r" blocked at="private+156" hot="private" />
      <Text x={1320} y={640} w={510} size={21} tone="bad" at="range">
        {"Decided by address range alone: a machine on your\nnetwork with a public address counts as public."}
      </Text>
      <Node id="dns" x={600} y={690} w={360} h={110} title="Your Mac's DNS resolvers" sub="private answers are dropped" tone="device" at="dns" />
      <Edge from="stack.b+60" to="dns.t+60" label="DNS" at="dns+8" />
      <Node x={1240} y={720} w={590} h={150} title="No allowlist" sub={"anything readable can go to any public host;\nno traffic limit · DNS is an open channel"} icon={TriangleAlert} tone="bad" at="open" />
    </>
  ),
};

const ports: Visual = {
  render: () => (
    <>
      <Group x={420} y={210} w={1410} h={690} label="Your Mac" tone="device" />
      <Group x={1390} y={320} w={390} h={220} label="Computer" tone="guest" solid />
      <Node id="server" x={1420} y={380} w={330} h={110} mono title=":3000" sub="a dev server" tone="guest" at="publish" />
      <Node id="loop" x={900} y={380} w={380} h={110} mono title="127.0.0.1:49152" sub="loopback only" tone="device" at="publish+50" hot="publish who" />
      <Edge from="loop" to="server" label="published" at="publish+56" hot="publish" flow labelShift={[0, -26]} />
      <Node id="url" x={470} y={650} w={760} h={96} mono size={25} title="http://3000.main.silo.localhost:47383" tone="engine" at="url" />
      <Text x={470} y={762} w={760} size={22} soft at="url+50">
        port · computer name · silo.localhost
      </Text>
      <Node id="proxy" x={1340} y={650} w={440} h={96} title="URL proxy" sub="one, inside the engine" tone="engine" at="proxy" />
      <Edge from="url" to="proxy" at="proxy+8" />
      <Edge from="proxy.t-120" to="loop.b" at="proxy+30" hot="proxy" />
      <Node id="local" x={470} y={270} w={360} h={90} title="Any program on your Mac" icon={Laptop} tone="ok" at="who" />
      <Edge from="local.b" to="loop.l-20" tone="ok" at="who+8" />
      <Node id="other" x={90} y={390} w={260} h={96} title="Other machines" at="who+70" hot="who" />
      <Edge from="other.r" to="loop.l+20" blocked at="who+76" hot="who" />
    </>
  ),
};

const secrets: Visual = {
  render: () => (
    <>
      <Node id="kc" x={800} y={200} w={380} h={110} title="Keychain" sub="value: sk-live-…9f2" icon={KeyRound} tone="device" at="store" hot="store swap never" />
      <Chips x={1220} y={212} w={600} size={21} at="store+70" hot="store" stagger={50} items={["assigned to: main", "allowed host: api.example.com"]} />

      <Group id="vm" x={90} y={400} w={480} h={340} label="The computer" tone="guest" solid />
      <Node id="env" x={125} y={600} w={410} h={96} mono size={22} title="API_KEY=$MSB_API_KEY" sub="a placeholder" tone="guest" at="placeholder" />
      <Node id="agent" x={125} y={460} w={410} h={96} title="Agent" icon={Bot} tone="guest" at="placeholder" hot="send" />

      <Node id="proxy" x={800} y={450} w={380} h={120} title="Proxy" sub="on your Mac" icon={Cpu} tone="engine" at="send" hot="send intercept swap" />
      <Edge from="agent" to="proxy" label="Bearer $MSB_API_KEY" at="send+10" hot="send intercept" flow labelShift={[0, -28]} />
      <Chip x={770} y={596} text="host is on the secret's list: intercept" tone="engine" at="intercept" />

      <Node id="api" x={1440} y={450} w={390} h={120} mono size={24} title="api.example.com" icon={Globe} at="swap" />
      <Edge from="kc" to="proxy" tone="device" at="swap+30" hot="swap" />
      <Edge from="proxy" to="api" tone="ok" label="Bearer sk-live-…9f2" at="swap+70" hot="swap" flow labelShift={[0, -28]} />
      <Chip x={1480} y={596} text="certificate verified first" tone="ok" at="swap+10" hot="swap" />
      <Text x={1250} y={650} w={580} size={22} soft align="right" at="swap+150">
        headers only: never a query string or a body
      </Text>

      <Edge from="kc.l" to={{ x: 330, y: 400, side: "t" }} blocked label="never enters" at="never" />
    </>
  ),
};

const secretLimits: Visual = {
  render: () => (
    <>
      <Node id="agent" x={90} y={250} w={280} h={96} title="Agent" icon={Bot} tone="guest" at="scope" hot="*" />
      <Node id="proxy" x={560} y={250} w={320} h={96} title="Proxy" tone="engine" at="scope" hot="*" />
      <Edge from="agent" to="proxy" at="scope+6" hot="*" flow="always" />
      <Node id="s" x={1200} y={200} w={600} h={84} title="The secret's hosts, and GitHub" sub="intercepted" tone="engine" at="scope+30" hot="scope" />
      <Node id="e" x={1200} y={312} w={600} h={84} title="Other HTTPS traffic" sub="passes with the server's own certificate" at="scope+110" hot="scope" />
      <Edge from="proxy.r-16" to="s.l" tone="engine" at="scope+36" hot="scope" />
      <Edge from="proxy.r+16" to="e.l" at="scope+116" hot="scope" />
      <List
        x={90}
        y={480}
        w={820}
        gap={34}
        size={30}
        items={[
          { at: "use", text: "Usable, not stealable", sub: "it can spend your credits; it cannot take the key elsewhere", icon: CircleCheck, tone: "ok" },
          { at: "echo", text: "Header echo", sub: "an allowed host that reflects headers reveals the value", icon: TriangleAlert, tone: "bad" },
        ]}
      />
      <List
        x={1000}
        y={480}
        w={830}
        gap={34}
        size={30}
        items={[
          { at: "any", text: "“Any host” assignment", sub: "after an explicit warning: no boundary for that secret", icon: TriangleAlert, tone: "bad" },
          { at: "other", text: "Sent to another host", sub: "the placeholder goes out unchanged; the macOS gateway blocks it" },
          { at: "log", text: "Usage log", sub: "name · host · count, and blocked attempts. Never values", icon: CircleCheck, tone: "ok" },
        ]}
      />
    </>
  ),
};

const github: Visual = {
  render: () => (
    <>
      <Node id="you" x={90} y={206} w={330} h={90} title="You, in a browser" tone="device" at="signin" />
      <Node id="gh0" x={540} y={206} w={280} h={90} title="GitHub" at="signin+40" hot="signin" />
      <Node id="kc" x={960} y={206} w={400} h={90} title="Keychain: your token" icon={KeyRound} tone="device" at="signin+150" hot="signin mint" />
      <Edge from="you" to="gh0" label="sign in once" at="signin+46" hot="signin" labelShift={[0, -24]} />
      <Edge from="gh0" to="kc" at="signin+156" hot="signin" />

      <Text x={90} y={344} size={23} soft at="grant">COMPUTER: main</Text>
      <Grid
        x={90}
        y={376}
        columns={[400, 190, 190]}
        head={["repository", "read", "write"]}
        rowHeight={52}
        rows={[
          { at: "grant", cells: ["acme/app", { text: "yes", tone: "ok" }, { text: "yes", tone: "engine" }] },
          { at: "grant+50", hot: "grant", cells: ["acme/docs", { text: "yes", tone: "ok" }, "off"] },
          { at: "grant+80", hot: "grant", cells: ["acme/infra", "not granted", "off"] },
        ]}
      />
      <Text x={90} y={590} w={780} size={21} soft at="grant+120">
        or every repository of one owner, with its own switch
      </Text>
      <Node id="eng" x={960} y={400} w={400} h={110} title="Engine" sub="asks GitHub for child tokens" icon={Cpu} tone="engine" at="mint" />
      <Edge from="kc" to="eng" at="mint+8" />
      <Node id="t1" x={1470} y={370} w={360} h={76} title="read: app, docs" tone="ok" at="mint+60" hot="mint pick" />
      <Node id="t2" x={1470} y={464} w={360} h={76} title="write: app" tone="engine" at="mint+90" hot="mint pick" />
      <Edge from="eng.r-14" to="t1.l" at="mint+64" hot="mint" />
      <Edge from="eng.r+14" to="t2.l" at="mint+94" hot="mint" />

      <Group x={90} y={660} w={600} h={210} label="The computer" tone="guest" solid at="guest" />
      <Node id="env" x={120} y={716} w={540} h={100} mono size={22} title="GH_TOKEN=$MSB_SILO_GITHUB" sub="a placeholder; git works normally" tone="guest" at="guest" />
      <Node id="proxy" x={860} y={716} w={340} h={100} title="Proxy" sub="picks the token per request" tone="engine" at="pick" />
      <Node id="gh" x={1400} y={716} w={430} h={100} title="github.com" sub="enforces the scope" at="pick+70" hot="pick" />
      <Edge from="env" to="proxy" at="pick+8" flow />
      <Edge from="proxy" to="gh" tone="ok" at="pick+76" hot="pick" flow />
      <Edge from="t2.b" to="proxy.t" dashed at="pick+30" hot="pick" />
      <Chip x={1010} y={840} text="write includes workflows: the agent can change that repository's CI" tone="bad" at="workflows" />
    </>
  ),
};

const pushToken: Visual = {
  render: () => (
    <>
      <Group x={90} y={220} w={440} h={210} label="The computer" tone="guest" solid />
      <Node id="commits" x={125} y={280} w={370} h={100} title="Commits" sub="in the workspace" tone="guest" at="push" />
      <Node id="eng" x={720} y={270} w={420} h={120} title="Engine, on your Mac" sub="bundled Git, no guest hooks" icon={Cpu} tone="engine" at="push" />
      <Node id="gh" x={1370} y={270} w={440} h={120} title="GitHub" sub="one branch" at="push+110" hot="push" />
      <Edge from="eng" to="commits" label="fetch" at="push+50" hot="push" labelShift={[0, -24]} />
      <Edge from="eng" to="gh" tone="ok" label="push" at="push+116" hot="push" labelShift={[0, -24]} />
      <Chips
        x={720}
        y={420}
        w={1100}
        size={22}
        at="single"
        stagger={45}
        items={["signed in: a token for this one repository", "revoked afterwards", "the guest's Git configuration never runs here"]}
      />

      <Node id="pat" x={90} y={650} w={500} h={130} size={28} title="Personal access token" sub="simpler, weaker" icon={KeyRound} tone="bad" at="pat" />
      <Node id="full" x={780} y={650} w={560} h={130} title="Nothing is scoped" sub="the agent, and Push, use its full permissions" tone="bad" at="full" />
      <Edge from="pat" to="full" tone="bad" at="full+8" />
      <Text x={1400} y={672} w={430} size={24} soft at="full+60">
        {"Chosen per computer.\nOutside what Silo's policy covers."}
      </Text>
    </>
  ),
};

const ssh: Visual = {
  render: () => (
    <>
      <Group x={430} y={220} w={860} h={600} label="Your Mac" tone="device" />
      <Group x={950} y={300} w={300} h={200} label="Computer" tone="guest" solid />
      <Node id="silo" x={975} y={356} w={250} h={96} title="user: silo" tone="guest" at="enable" />
      <Node id="listener" x={490} y={340} w={370} h={130} title="SSH listener" sub={"its own key\n127.0.0.1:2222"} tone="engine" at="listener" />
      <Edge from="listener" to="silo" at="listener+10" />
      <Node id="tool" x={490} y={600} w={370} h={100} title="An agent or a tool" sub="on this Mac" tone="device" at="enable" hot="enable listener" />
      <Edge from="tool" to="listener" at="listener+40" hot="listener" flow />
      <Node id="far" x={90} y={350} w={260} h={110} title="Another machine" at="lan" />
      <Edge from="far" to="listener" label="one chosen address" at="lan+30" hot="lan" labelShift={[0, -26]} />
      <Text x={90} y={480} w={300} size={22} soft at="lan+60">
        only after a confirmation
      </Text>
      <List
        x={1370}
        y={300}
        w={470}
        gap={22}
        items={[
          { at: "end", text: "Turned off: sessions end" },
          { at: "end+40", hot: "end", text: "Computer stopped: sessions end" },
          { at: "end+80", hot: "end", text: "A connection never starts a computer" },
        ]}
      />
    </>
  ),
};

const localTrust: Visual = {
  render: () => (
    <>
      <Group x={90} y={210} w={1740} h={470} label="Your user account" tone="device" />
      <Node id="app" x={150} y={270} w={300} h={84} title="Silo app" tone="device" at="anyone" />
      <Node id="cli" x={150} y={384} w={300} h={84} title="silo CLI" tone="device" at="anyone+10" hot="anyone" />
      <Node id="mal" x={150} y={520} w={300} h={110} title="Any other process" sub="malware included" tone="bad" at="anyone+70" hot="anyone malware" />
      <Node id="sock" x={640} y={384} w={340} h={84} mono title="engine.sock" at="anyone" hot="anyone noauth" />
      <Node id="silod" x={1150} y={372} w={280} h={108} mono size={30} title="silod" icon={Cpu} tone="engine" at="anyone" hot="*" />
      <Edge from="app" to="sock.l-20" at="anyone+6" />
      <Edge from="cli" to="sock" at="anyone+16" hot="anyone" />
      <Edge from="mal" to="sock.l+20" tone="bad" at="anyone+76" hot="anyone malware" flow />
      <Edge from="sock" to="silod" at="anyone+20" hot="*" />
      <Chips x={600} y={494} w={460} size={21} tone="bad" at="noauth" stagger={40} items={["no client identity", "no audit log"]} />
      <Node id="msb" x={1560} y={372} w={230} h={108} title="Runtime" sub="runs as you" at="escape" />
      <Edge from="silod" to="msb" at="escape+6" />
      <Chip x={1380} y={520} text="an escape lands here: no second sandbox" tone="bad" at="escape+70" hot="escape" />
      <Chips
        x={150}
        y={730}
        w={1600}
        size={24}
        tone="bad"
        at="malware+70"
        hot="malware"
        stagger={36}
        items={["ran commands in a computer", "saved a secret", "published a port"]}
      />
      <Text x={150} y={820} w={1600} size={26} soft at="scope">
        Out of scope today. Client identity and an audit log are on the roadmap.
      </Text>
    </>
  ),
};

const evals: Visual = {
  render: () => (
    <>
      <Text x={90} y={196} w={1700} size={34} weight={600} at="ten">
        10 scenarios. A probe plays the root attacker.
      </Text>
      <Node id="try" x={90} y={280} w={420} h={90} title="The attempt" sub="must be blocked" tone="bad" at="control" />
      <Node id="ctl" x={560} y={280} w={420} h={90} title="Its control" sub="must succeed" tone="ok" at="control+40" hot="control" />
      <Text x={1030} y={308} w={600} size={24} soft at="control+90">
        otherwise: inconclusive
      </Text>
      <List
        x={90}
        y={430}
        w={800}
        gap={9}
        size={26}
        items={[
          { at: "pass+12", hot: "pass", text: "Host files", icon: CircleCheck, tone: "ok" },
          { at: "pass+32", hot: "pass", text: "Host network", icon: CircleCheck, tone: "ok" },
          { at: "pass+56", hot: "pass", text: "Computer to computer", icon: CircleCheck, tone: "ok" },
          { at: "pass+92", hot: "pass", text: "Secret in the guest", icon: CircleCheck, tone: "ok" },
          { at: "pass+128", hot: "pass", text: "Secret to other hosts", icon: CircleCheck, tone: "ok" },
          { at: "pass+166", hot: "pass", text: "GitHub scope", icon: CircleCheck, tone: "ok" },
          { at: "pass+196", hot: "pass", text: "Engine socket, other users (permissions read, not attacked)", icon: CircleCheck, tone: "ok" },
        ]}
      />
      <List
        x={1000}
        y={430}
        w={830}
        gap={9}
        size={26}
        items={[
          { at: "gaps+70", hot: "gaps", text: "Reflected secret: leaks", icon: TriangleAlert, tone: "bad" },
          { at: "gaps+110", hot: "gaps", text: "Open egress: a file was posted out", icon: TriangleAlert, tone: "bad" },
          { at: "gaps+145", hot: "gaps", text: "Same-user client: full control", icon: TriangleAlert, tone: "bad" },
        ]}
      />
      <Node x={1000} y={640} w={830} h={190} align="left" title="Covered: Linux computers on a Mac" sub={"Not covered: macOS computers · containers ·\nLinux hosts · personal tokens\nLast runs: 3 November 2026, engine 0.1.1, protocol 6"} at="cover" />
    </>
  ),
};

export const isolation = {
  threat,
  network,
  ports,
  secrets,
  "secret-limits": secretLimits,
  github,
  "push-token": pushToken,
  ssh,
  "local-trust": localTrust,
  evals,
};
