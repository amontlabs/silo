import React from "react";
import { Bot, CircleX, Cpu, GitBranch, GitMerge, Laptop, MousePointerClick, Trash2, TriangleAlert } from "lucide-react";
import { Chip, Chips, Code, Edge, Group, List, Node, Text } from "../kit.tsx";
import type { Visual } from "./index.ts";

const app: Visual = {
  render: () => (
    <>
      <Group x={90} y={210} w={1090} h={690} label="Silo" tone="device" solid />
      <Node x={120} y={250} w={330} h={64} align="left" title="Pending changes" at="sidebar" hot="pending" />
      <Node x={120} y={326} w={330} h={64} align="left" title="GitHub" at="sidebar" hot="access" />
      <Node x={120} y={402} w={330} h={64} align="left" title="Secrets" at="sidebar" hot="access" />
      <Node x={120} y={498} w={330} h={64} align="left" title="Computers" tone="device" at="sidebar" hot="computers" />
      <Text x={146} y={572} w={320} size={21} soft at="computers">
        {"All computers · Files · Logs\nNetwork · Activity"}
      </Text>
      <Node x={120} y={660} w={330} h={64} align="left" title="Settings" at="sidebar" hot="settings" />
      <Text x={146} y={734} w={320} size={21} soft at="settings">
        {"General · Connections\nNotifications"}
      </Text>
      <Node x={500} y={250} w={640} h={84} align="left" title="main" sub="Linux · running" icon={Bot} tone="guest" at="sidebar" hot="computers" />
      <Node x={500} y={350} w={640} h={84} align="left" title="fix-login" sub="fork of main · changes pending" icon={Bot} tone="guest" at="sidebar" hot="pending" />
      <Text x={500} y={500} size={21} soft at="onboard">FIRST LAUNCH</Text>
      <Chips x={500} y={536} w={640} size={22} at="onboard+40" hot="onboard" stagger={28} items={["1 Dependencies", "2 A first computer", "3 GitHub", "4 Review"]} />

      <Node x={1260} y={220} w={570} h={110} title="Menu bar panel" sub="your computers, start and stop" at="tray" />
      <Node x={1260} y={390} w={275} h={110} title="Activity" sub="operations" at="history" />
      <Node x={1555} y={390} w={275} h={110} title="Logs" sub="kept 7 days" at="history+70" hot="history" />
      <Node id="bundle" x={1260} y={590} w={570} h={90} title="One React bundle" at="fixtures" />
      <Node id="prod" x={1260} y={760} w={275} h={90} title="The engine" sub="in production" tone="engine" at="fixtures+40" hot="fixtures" />
      <Node id="fix" x={1555} y={760} w={275} h={90} title="Fixtures" sub="design, tests" at="fixtures+90" hot="fixtures" />
      <Edge from="bundle.b-100" to="prod.t" at="fixtures+44" hot="fixtures" />
      <Edge from="bundle.b+100" to="fix.t" at="fixtures+94" hot="fixtures" />
    </>
  ),
};

const cli: Visual = {
  render: () => (
    <>
      <Code
        x={90}
        y={210}
        w={880}
        h={480}
        size={24}
        title="silo"
        at="binary"
        lines={[
          { at: "basics", text: "$ silo create dev --linux" },
          { at: "basics+22", text: "$ silo start main" },
          { at: "basics+40", text: "$ silo exec main -- npm test" },
          { at: "basics+62", text: "$ silo checkpoint main" },
          { at: "basics+86", text: "$ silo publish main 3000 --http" },
          { at: "basics+104", text: "$ silo export main" },
          { text: "" },
          { at: "json", text: "$ silo list --json", tone: "engine" },
          { at: "json+70", text: "$ silo job <id>        # wait again", tone: "engine" },
        ]}
      />
      <Chip x={90} y={716} text="installed by the app, in ~/.local/bin" at="binary+60" hot="binary" />
      <Code
        x={1040}
        y={210}
        w={790}
        h={350}
        size={22}
        title=".silo/recipe.yaml"
        at="project"
        lines={[
          { at: "recipe", text: "os: linux" },
          { at: "recipe+30", text: "packages:" },
          { at: "recipe+30", text: "  apt: [build-essential, jq]" },
          { at: "recipe+65", text: "setup:" },
          { at: "recipe+65", text: "  - git clone https://example.com/app /opt/app" },
          { at: "recipe+100", text: "services:" },
          { at: "recipe+100", text: "  web: python3 -m http.server 8080" },
          { at: "recipe+125", text: "ports: [8080]" },
        ]}
      />
      <Code
        x={1040}
        y={600}
        w={790}
        h={260}
        size={22}
        title=".silo/silo.yaml"
        at="rules"
        lines={[
          { at: "rules+50", text: "fork_secrets: [OPENAI_API_KEY]" },
          { at: "rules+110", text: "fold: ask" },
          { at: "rules+110", text: "auto_keep:" },
          { at: "rules+110", text: "  - when: { only: [code], tests: pass }" },
        ]}
      />
    </>
  ),
};

const up: Visual = {
  render: () => (
    <>
      <Node id="recipe" x={90} y={250} w={380} h={100} mono title=".silo/recipe.yaml" at="build" />
      <Node id="builder" x={600} y={250} w={420} h={100} title="A hidden computer" sub="packages · setup · services" tone="guest" at="build+30" hot="build" />
      <Node id="cp" x={1150} y={250} w={480} h={100} title="Checkpoint" sub="recipe <name> <hash>" at="build+90" hot="build cache up" />
      <Edge from="recipe" to="builder" label="build" at="build+34" hot="build" labelShift={[0, -24]} />
      <Edge from="builder" to="cp" at="build+94" hot="build" />
      <Chips x={1150} y={372} w={680} size={22} tone="engine" at="cache" stagger={70} items={["key: hash(recipe + image + builder version)", "any of them changes: rebuild"]} />

      <Node id="cmd" x={90} y={600} w={300} h={110} mono size={30} title="silo up" tone="engine" at="up" />
      <Node id="main" x={1150} y={590} w={480} h={130} size={32} title="main" sub="your base computer" icon={Bot} tone="guest" at="up+30" hot="up main" />
      <Edge from="cp.b" to="main.t" label="fork" at="up+36" hot="up" />
      <List
        x={500}
        y={560}
        w={560}
        gap={12}
        size={26}
        items={[
          { at: "up+30", hot: "up", text: "Forked from the checkpoint" },
          { at: "up+70", hot: "up", text: "Started" },
          { at: "up+95", hot: "up", text: "Ports published" },
          { at: "up+125", hot: "up", text: "ssh main.silo set up" },
        ]}
      />
    </>
  ),
};

const forkLoop: Visual = {
  render: () => (
    <>
      <Node id="main" x={90} y={430} w={300} h={120} size={32} title="main" sub="your base" icon={Bot} tone="guest" at="idea" hot="idea decide" />
      <Node id="f1" x={560} y={270} w={320} h={90} title="fork: fix-login" tone="guest" at="fork" hot="fork work" />
      <Node id="f2" x={560} y={445} w={320} h={90} title="fork: upgrade" tone="guest" at="fork+40" hot="fork work" />
      <Node id="f3" x={560} y={620} w={320} h={90} title="fork: refactor" tone="guest" at="fork+70" hot="fork work" />
      <Edge from="main.r-20" to="f1.l" at="fork+6" />
      <Edge from="main.r" to="f2.l" at="fork+46" hot="fork" />
      <Edge from="main.r+20" to="f3.l" at="fork+76" hot="fork" />
      <Chip x={910} y={292} text="agent" tone="guest" at="work" />
      <Chip x={910} y={467} text="agent" tone="guest" at="work+10" hot="work" />
      <Chip x={910} y={642} text="agent" tone="guest" at="work+20" hot="work" />
      <Text x={560} y={372} w={600} size={20} mono soft at="steps">
        #1 npm install   #2 npm test   #3 …
      </Text>
      <Node id="diff" x={1130} y={430} w={280} h={120} mono size={28} title="silo diff" sub="what changed" at="diff" />
      <Edge from="f1.r" to="diff.t-60" at="diff+8" />
      <Edge from="f2.r" to="diff.l" at="diff+12" />
      <Edge from="f3.r" to="diff.b-60" at="diff+16" />
      <Node id="fold" x={1530} y={290} w={300} h={110} title="fold" sub="keep into main" icon={GitMerge} tone="ok" at="decide+40" hot="decide" />
      <Node id="drop" x={1530} y={580} w={300} h={110} title="drop" sub="throw the fork away" icon={Trash2} tone="bad" at="decide+110" hot="decide" />
      <Edge from="diff.r-20" to="fold.l" at="decide+46" hot="decide" />
      <Edge from="diff.r+20" to="drop.l" at="decide+116" hot="decide" />
      <Edge from="fold.t" to="main.t" tone="ok" label="the changes" at="decide+70" hot="decide" />
      <Node id="req" x={1010} y={790} w={400} h={90} title="fold --request: nothing kept" at="request" />
      <Node id="you" x={1560} y={790} w={270} h={90} title="You approve" icon={Laptop} tone="device" at="request+60" hot="request" />
      <Edge from="req" to="you" at="request+66" hot="request" />
    </>
  ),
};

const diff: Visual = {
  render: () => (
    <>
      <Node id="base" x={90} y={230} w={380} h={90} title="Record at fork time" at="whole" />
      <Node id="now" x={90} y={360} w={380} h={90} title="The fork, now" tone="guest" at="whole+50" hot="whole" />
      <Node id="diff" x={590} y={290} w={220} h={100} mono size={30} title="diff" tone="engine" at="whole+90" hot="whole noise" />
      <Edge from="base.r" to="diff.l-16" at="whole+94" hot="whole" />
      <Edge from="now.r" to="diff.l+16" at="whole+98" hot="whole" />
      <Chips
        x={900}
        y={240}
        w={930}
        size={26}
        tone="ok"
        at="buckets"
        stagger={22}
        items={["Code", "Packages", "Settings and files", "Databases", "Other"]}
      />
      <Text x={900} y={400} size={22} tone="bad" weight={600} at="notkept">REPORTED, NEVER KEPT</Text>
      <Chips x={900} y={440} w={930} size={24} tone="bad" at="notkept+30" hot="notkept" stagger={24} items={["new processes", "listening ports", "services", "credential files"]} />
      <Node id="idle" x={90} y={600} w={460} h={100} title="An idle fork" sub="of the same checkpoint" at="noise" />
      <Edge from="idle.t+140" to="diff.b" label="silo diff --baseline" at="noise+40" hot="noise" />
      <Node x={900} y={620} w={450} h={130} align="left" title="Credentials used" sub={"OPENAI_API_KEY\napi.openai.com · 12 requests"} at="creds" />
      <Node x={1380} y={620} w={450} h={130} align="left" title="Hosted reviewer" sub={"optional · off unless configured\nadvises only"} tone="ghost" at="reviewer" />
    </>
  ),
};

const fold: Visual = {
  render: () => (
    <>
      <Node id="base" x={730} y={200} w={460} h={80} title="Where the fork started" at="three" />
      <Node id="fork" x={300} y={320} w={340} h={80} title="The fork" tone="guest" at="three+40" hot="three" />
      <Node id="target" x={1280} y={320} w={340} h={80} title="The target: main" tone="guest" at="three+70" hot="three" />
      <Node id="merge" x={810} y={440} w={300} h={90} mono size={28} title="fold" icon={GitMerge} tone="engine" at="three" />
      <Edge from="base.l" to="fork.t" at="three+44" hot="three" />
      <Edge from="base.r" to="target.t" at="three+74" hot="three" />
      <Edge from="fork.b" to="merge.l" at="three+48" hot="three" />
      <Edge from="target.b" to="merge.r" at="three+78" hot="three" />
      <List
        x={90}
        y={590}
        w={860}
        gap={18}
        size={27}
        items={[
          { at: "pinned", text: "Pinned to the diff you reviewed", sub: "the fork changed since: refused" },
          { at: "safe", text: "Checkpoint first, restore on failure", sub: "and silo undo goes back later", tone: "ok" },
          { at: "refuse", text: "Refuses", sub: "conflicts · credentials in Git history · uncovered database changes", icon: CircleX, tone: "bad" },
        ]}
      />
      <List
        x={1020}
        y={590}
        w={810}
        gap={18}
        size={27}
        items={[
          { at: "writeback", text: "Write-back", sub: "added packages go into the recipe" },
          { at: "auto", text: "auto_keep rules", sub: "code only, tests passing: but the fork runs those tests" },
          { at: "promote", text: "Promote", sub: "the fork becomes the new main", icon: GitBranch },
        ]}
      />
    </>
  ),
};

const folderPr: Visual = {
  render: () => (
    <>
      <Node id="repo" x={90} y={250} w={440} h={110} mono title="./repo" sub="on your Mac · committed history only" tone="device" at="folder" hot="folder back" />
      <Node id="c" x={820} y={250} w={420} h={110} title="A fresh computer" sub="the agent works here" icon={Bot} tone="guest" at="folder+50" hot="folder" />
      <Edge from="repo.r-22" to="c.l-22" label="silo fork ./repo" at="folder+56" hot="folder" labelShift={[0, -26]} />
      <Edge from="c.l+22" to="repo.r+22" label="silo fold" tone="ok" at="back" labelShift={[0, 26]} />
      <Chips x={90} y={390} w={700} size={22} at="back+50" hot="back" stagger={30} items={["commits come back", "backup reference", "then merge"]} />

      <Node id="push" x={820} y={620} w={420} h={110} title="Your Mac pushes a branch" tone="device" at="pr" />
      <Node id="pr" x={1410} y={620} w={420} h={110} title="A pull request" sub="its text: the review summary" tone="ok" at="pr+70" hot="pr" />
      <Edge from="c.b" to="push.t" label="silo fold --pr" at="pr+8" />
      <Edge from="push" to="pr" at="pr+76" hot="pr" />
      <Chip x={820} y={760} text="your own GitHub credentials, on the host" tone="ok" at="own" />
      <Chip x={1300} y={760} text="never the fork's" tone="bad" at="own+70" hot="own" />
    </>
  ),
};

const mcp: Visual = {
  render: () => (
    <>
      <Node id="agent" x={90} y={250} w={400} h={110} title="An agent on your Mac" icon={Bot} tone="device" at="serve" />
      <Node id="mcp" x={640} y={250} w={340} h={110} mono size={28} title="silo mcp" sub="39 tools" tone="engine" at="serve" />
      <Node id="silod" x={1130} y={250} w={260} h={110} mono size={28} title="silod" icon={Cpu} tone="engine" at="serve" hot="*" />
      <Node id="forks" x={1540} y={250} w={290} h={110} title="Forks" icon={Bot} tone="guest" at="serve" hot="*" />
      <Edge from="agent" to="mcp" at="serve+8" flow="always" />
      <Edge from="mcp" to="silod" at="serve+12" hot="*" />
      <Edge from="silod" to="forks" at="serve+16" hot="*" />
      <Chips x={90} y={450} w={1060} mono size={23} at="flow" stagger={12} items={["fork", "exec", "diff", "fold", "fold { request }", "steps_list", "tree", "checkpoint", "publish"]} />
      <Chips
        x={90}
        y={590}
        w={1060}
        mono
        size={23}
        tone="guest"
        at="gui"
        stagger={12}
        items={["fork_gui", "computer_use_screenshot", "computer_use_click", "computer_use_type", "computer_use_scroll"]}
      />
      <Node x={90} y={740} w={1060} h={120} align="left" title="Everything else: your authority at the command line" sub="keep · promote · undo · exec in main. Narrowed only by .silo/silo.yaml" icon={TriangleAlert} tone="bad" at="power" />
      <List
        x={1260}
        y={440}
        w={570}
        gap={16}
        items={[
          { at: "limits", text: "No secret values", icon: CircleX, tone: "bad" },
          { at: "limits+30", hot: "limits", text: "No export or import", icon: CircleX, tone: "bad" },
          { at: "limits+75", hot: "limits", text: "Cannot drop main", icon: CircleX, tone: "bad" },
          { at: "limits+100", hot: "limits", text: "Cannot drive main's desktop", icon: MousePointerClick, tone: "bad" },
        ]}
      />
    </>
  ),
};

const inGuest: Visual = {
  render: () => (
    <>
      <Group x={90} y={220} w={680} h={420} label="A computer with a resident agent" tone="guest" solid />
      <Node id="agent" x={130} y={290} w={280} h={96} title="Agent" icon={Bot} tone="guest" at="why" />
      <Node id="client" x={450} y={290} w={280} h={96} mono size={22} title="silo --in-guest" tone="guest" at="why+40" hot="why" />
      <Edge from="agent" to="client" at="why+46" hot="why" />
      <Node id="token" x={130} y={470} w={280} h={96} title="Scoped token" sub="installed by you" at="token" />
      <Node id="bridge" x={450} y={470} w={280} h={96} mono size={21} title="/.msb/silo-bridge" sub="shared folder" at="token+60" hot="token" />
      <Edge from="client" to="bridge" at="token+66" hot="token" />
      <Node id="silod" x={900} y={464} w={280} h={108} mono size={30} title="silod" sub="checks the token" icon={Cpu} tone="engine" at="token+60" hot="token" />
      <Edge from="bridge" to="silod" at="token+90" hot="token" flow />
      <Text x={1300} y={200} size={22} weight={600} tone="ok" at="allowed">13 METHODS</Text>
      <List
        x={1300}
        y={240}
        w={530}
        gap={10}
        size={26}
        items={[
          { at: "allowed+20", hot: "allowed", text: "List its computers", tone: "ok" },
          { at: "allowed+40", hot: "allowed", text: "Fork itself", tone: "ok" },
          { at: "allowed+70", hot: "allowed", text: "Start and stop its own forks", tone: "ok" },
          { at: "allowed+115", hot: "allowed", text: "Diff", tone: "ok" },
          { at: "allowed+135", hot: "allowed", text: "Request review", tone: "ok" },
        ]}
      />
      <Chip x={1300} y={500} text="at most 32 forks · 3 running" at="caps" />
      <List
        x={1300}
        y={590}
        w={530}
        gap={10}
        size={26}
        items={[
          { at: "denied", text: "Keep or delete", icon: CircleX, tone: "bad" },
          { at: "denied+30", hot: "denied", text: "Run commands elsewhere", icon: CircleX, tone: "bad" },
          { at: "denied+60", hot: "denied", text: "Change access", icon: CircleX, tone: "bad" },
        ]}
      />
      <Node id="pending" x={840} y={720} w={400} h={110} title="Pending changes" sub="waits for you" icon={Laptop} tone="device" at="denied+110" hot="denied" />
      <Edge from="silod" to="pending" at="denied+116" hot="denied" />
    </>
  ),
};

export const working = { app, cli, up, "fork-loop": forkLoop, diff, fold, "folder-pr": folderPr, mcp, "in-guest": inGuest };
