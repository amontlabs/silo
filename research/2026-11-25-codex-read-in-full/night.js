// @exec: {"yield_time_ms": 20000}
const files = ["a.txt", "b.txt"];
for (const f of files) {
  await tools.exec_command({cmd: "echo made-in-a-loop > " + f});
}
const verb = ["mv", "-v"].join(" ");
const r = await tools.exec_command({cmd: `${verb} b.txt b.old; ls; exit 3`});
await tools.apply_patch("*** Begin Patch\n*** Add File: c.txt\n+made by a patch\n*** End Patch\n");
text(r.output);
