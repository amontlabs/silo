/* The page around the film. Under it, in this order, each part only when film.json has it:
   Was it clear? (one line per scene), Test the picture, What the film shows (the entries and sentences of the
   plan named in `shows`, printed from plan/ at build time: an entry not decided shows under its sentence every
   sentence a yes would set and every id it would remove, then agree / change / leave open and a note; one in force
   or dropped has none), Seen this go wrong?, A choice, Words.
   Then a button that copies every answer as plain text, naming each entry by its id.
   Each part is a <section data-part="…">; check.sh looks for them. Answers are kept in localStorage under the
   film's id, the answers on entries under `entries`, by id. It runs after explainer.js, so the scenes'
   captions are known (window.__spec). */
(() => {
  const C = window.FILM, $ = s => document.querySelector(s);
  const KEY = "silo-walkthrough:" + C.id;
  const open = e => e.status === "not decided";
  const captions = (window.__spec && window.__spec.captions) || [];

  let A = {};
  try { A = JSON.parse(localStorage.getItem(KEY) || "{}") || {}; } catch (e) { A = {}; }
  // Answers to the statements of the older pages, by number, stay as they are under `statements`: those
  // statements are gone, and an answer by number cannot be carried to an entry. Answers to the promises of the
  // older pages stay under `promises` for the same reason: each promise is now an entry of the plan.
  for (const k of ["entries", "clear"]) A[k] = A[k] || {};
  A.choice = A.choice || { pick: "", note: "" };
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(A)); } catch (e) { /* kept for this visit only */ } };

  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
  let uid = 0;
  // A row of mutually exclusive buttons (radio inputs styled as a segmented control).
  function choices(label, opts, get, set) {
    const group = el("div", "choices"), name = "c" + (++uid);
    group.setAttribute("role", "radiogroup");
    group.setAttribute("aria-label", label);
    opts.forEach(([v, text]) => {
      const id = `${name}-${v}`, input = el("input");
      Object.assign(input, { type: "radio", name, id, value: v, checked: get() === v });
      input.onchange = () => { set(v); save(); };
      const lab = el("label", null, text);
      lab.htmlFor = id;
      group.append(input, lab);
    });
    return group;
  }
  function field(label, get, set, { rows = 2, placeholder = "Note (optional)" } = {}) {
    const t = el("textarea", "note");
    Object.assign(t, { rows, value: get() || "", placeholder });
    t.setAttribute("aria-label", label);
    t.oninput = () => { set(t.value); save(); };
    return t;
  }
  const parts = $(".parts");
  function part(name, title, intro) {
    const s = el("section", "part " + name);
    s.dataset.part = name;
    const h = el("h2", null, title);
    h.id = "h-" + name;
    s.setAttribute("aria-labelledby", h.id);
    s.appendChild(h);
    if (intro) s.appendChild(el("p", "intro", intro));
    parts.appendChild(s);
    return s;
  }
  const head = (i, mark) => { const top = el("div", "st-top"); top.append(el("span", "num", String(i + 1))); if (mark) top.append(el("span", "mark", mark)); return top; };

  // Was it clear?
  const scenes = C.clear ? (captions.length ? captions : (window.FILM_VOICE_DATA.voices[0].scenes || []).map((_, i) => `Scene ${i + 1}`)) : [];
  if (C.clear) {
    const s = part("clear", "Was it clear?", "One line per scene of the film.");
    const list = el("ol", "list");
    scenes.forEach((cap, i) => {
      const a = A.clear[i] || (A.clear[i] = { v: "", why: "" });
      const li = el("li", "st");
      li.append(head(i), el("p", "st-text", cap),
        choices(`Scene ${i + 1}`, [["clear", "Clear"], ["lost", "Lost me here"]], () => a.v, v => { a.v = v; }),
        field(`Why, scene ${i + 1}`, () => a.why, v => { a.why = v; }, { rows: 1, placeholder: "Why (optional)" }));
      list.appendChild(li);
    });
    s.appendChild(list);
  }

  // Test the picture
  if (C.test) {
    const s = part("test", "Test the picture");
    s.append(el("p", "st-text", C.test.question),
      field("Your answer", () => A.test, v => { A.test = v; }, { rows: 3, placeholder: "Your answer" }));
    const show = el("button", "ghost", "Show the answer"), ans = el("p", "given", C.test.answer);
    ans.hidden = !A.revealed;
    show.hidden = !!A.revealed;
    show.onclick = () => { A.revealed = true; save(); ans.hidden = false; show.hidden = true; };
    s.append(show, ans);
  }

  // What the film shows: the plan's entries and sentences, as plan/ held them when the page was built.
  {
    const s = part("shows", "What the film shows", "From the plan. You rule only on what is not decided.");
    const list = el("ol", "list");
    C.entries.forEach((e, i) => {
      const li = el("li", "st");
      li.dataset.id = e.id;
      li.dataset.status = e.status;
      const top = head(i, e.status);
      top.append(el("code", "id", e.id));
      li.append(top);
      if (e.asks) li.append(el("p", "asks", e.asks));
      li.append(el("p", "st-text", e.text));
      // A yes covers exactly what the entry shows: its sentence, what it sets and what it removes.
      if (open(e) && (e.sets.length || e.removes.length)) {
        const box = el("div", "sets");
        box.append(el("p", "sets-head", e.sets.length ? "A yes also sets:" : "A yes also removes:"));
        e.sets.forEach(x => {
          const row = el("p", "set");
          row.dataset.in = x.in;
          row.dataset.name = x.name;
          row.append(el("span", "set-in", x.in), " ", el("code", "set-name", x.name), " ", el("span", "set-text", x.text));
          box.append(row);
        });
        if (e.sets.length && e.removes.length) box.append(el("p", "sets-head", "And removes:"));
        e.removes.forEach(r => { const row = el("p", "set removes"); row.dataset.removes = r; row.append(el("code", "set-name", r)); box.append(row); });
        li.append(box);
      }
      if (open(e)) {
        const a = A.entries[e.id] || (A.entries[e.id] = { choice: "", note: "" });
        li.append(choices(e.id, [["agree", "Agree"], ["change", "Change"], ["open", "Leave open"]], () => a.choice, v => { a.choice = v; }),
          field(`Note on ${e.id}`, () => a.note, v => { a.note = v; }));
      }
      list.appendChild(li);
    });
    s.appendChild(list);
  }

  // Seen this go wrong?
  if (C.wrong) {
    const s = part("wrong", "Seen this go wrong?");
    s.append(el("p", "st-text", C.wrong), field("Your story", () => A.wrong, v => { A.wrong = v; }, { rows: 3, placeholder: "Your story (optional)" }));
  }

  // A choice
  if (C.choice && C.choice.options && C.choice.options.length) {
    const s = part("choice", "A choice");
    s.appendChild(el("p", "st-text", C.choice.question));
    const list = el("div", "options"), name = "choice";
    list.setAttribute("role", "radiogroup");
    list.setAttribute("aria-label", C.choice.question);
    C.choice.options.forEach(o => {
      const id = `opt-${o.key}`, input = el("input"), lab = el("label", "option");
      Object.assign(input, { type: "radio", name, id, value: o.key, checked: A.choice.pick === o.key });
      input.onchange = () => { A.choice.pick = o.key; save(); };
      lab.htmlFor = id;
      const top = el("span", "opt-top");
      top.append(el("span", "opt-key", o.key), el("span", "opt-text", o.text));
      if (o.recommended) top.append(el("span", "rec", "Recommended"));
      lab.append(top, el("span", "cost", "What it costs you: " + o.cost));
      list.append(input, lab);
    });
    s.append(list, field("Note on the choice", () => A.choice.note, v => { A.choice.note = v; }));
  }

  // Words
  {
    const s = part("words", "Words"), dl = el("dl");
    C.words.forEach(w => dl.append(el("dt", null, w.word), el("dd", null, w.means)));
    s.appendChild(dl);
  }

  // Copy my answers: every part, as plain text.
  function text() {
    const out = [`${C.title}: my answers`];
    const note = (n, pad = "   ") => ((n || "").trim() ? [`${pad}Note: ${n.trim().replace(/\n+/g, "\n" + pad)}`] : []);
    const name = { clear: "clear", lost: "lost me here", agree: "agree", change: "change", open: "leave open", "": "no answer" };
    if (C.clear) {
      out.push("", "Was it clear?");
      scenes.forEach((cap, i) => { const a = A.clear[i] || {}; out.push(`${i + 1}. [${name[a.v || ""]}] ${cap}`, ...((a.why || "").trim() ? [`   Why: ${a.why.trim()}`] : [])); });
    }
    if (C.test) out.push("", "Test the picture", C.test.question, `My answer: ${(A.test || "").trim() || "(none)"}`);
    const undecided = C.entries.filter(open);
    if (undecided.length) {
      out.push("", "What the film shows, not decided");
      undecided.forEach(e => { const a = A.entries[e.id] || {}; out.push(`${e.id}: [${name[a.choice || ""]}] ${e.text}`, ...note(a.note)); });
    }
    if (C.wrong) out.push("", "Seen this go wrong?", (A.wrong || "").trim() || "(nothing)");
    if (C.choice && C.choice.options && C.choice.options.length) {
      const o = C.choice.options.find(x => x.key === A.choice.pick);
      out.push("", "A choice", C.choice.question, o ? `My pick: ${o.key}. ${o.text}` : "My pick: none", ...note(A.choice.note, ""));
    }
    return out.join("\n") + "\n";
  }
  function fallback(t) {
    const ta = el("textarea");
    ta.value = t;
    ta.style.cssText = "position:fixed;left:-9999px;top:0";
    document.body.appendChild(ta);
    ta.select();
    let ok = false;
    try { ok = document.execCommand("copy"); } catch (e) { ok = false; }
    ta.remove();
    return ok;
  }
  $(".copy").onclick = async () => {
    const t = text(), say = $(".copied");
    let ok = false;
    try { await navigator.clipboard.writeText(t); ok = true; } catch (e) { ok = fallback(t); }
    say.textContent = ok ? "Copied." : "The browser refused to copy.";
  };
  window.__answersText = text; // for tests
})();
