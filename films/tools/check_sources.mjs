// check_sources.mjs <film dir>: the film has as many scenes as the narration, its title is film.json's, and
// captions are ten words or fewer. What the film shows is checked against plan/ by build.py and check_page.py.
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
// A film is a folder directly under films/ that holds a film.json, links resolved; nothing else is read.
const W = path.dirname(path.dirname(fs.realpathSync(new URL(import.meta.url))));
const ch = fs.realpathSync(process.argv[2]);
if (path.dirname(ch) !== W || ["kit", "player", "tools"].includes(path.basename(ch)) || path.basename(ch).startsWith(".")
    || !fs.existsSync(path.join(ch, "film.json"))) {
  console.log(`FAIL\n  ${process.argv[2]} is not a film: a film is a folder directly under ${W} that holds a film.json`);
  process.exit(1);
}
const film = JSON.parse(fs.readFileSync(path.join(ch, "film.json"), "utf8"));
const narration = JSON.parse(fs.readFileSync(path.join(ch, "narration.json"), "utf8"));
let spec = null;
vm.runInNewContext(fs.readFileSync(path.join(ch, "explainer.js"), "utf8"), { explainer: s => { spec = s; } });
const bad = [];
if (!Array.isArray(film.shows) || !film.shows.length) bad.push('film.json has no "shows": the ids of the plan\'s entries and statements it explains');
if (!spec) bad.push("explainer.js never called explainer({...})");
else {
  spec.scenes.forEach((s, i) => {
    const words = (s.caption || "").trim().split(/\s+/).length;
    if (words > 10) bad.push(`scene ${i + 1}: the caption has ${words} words (10 at most)`);
  });
  if (spec.scenes.length !== narration.scenes.length) bad.push(`the film has ${spec.scenes.length} scenes, the narration ${narration.scenes.length}`);
  if (spec.title !== film.title) bad.push(`the film's title "${spec.title}" differs from film.json's "${film.title}"`);
}
if (bad.length) { console.log("FAIL\n  " + bad.join("\n  ")); process.exit(1); }
console.log(`ok: ${spec.scenes.length} scenes, captions short, title as in film.json`);
