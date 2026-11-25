"""The morning screen: one page from morning.py's JSON.

    python3 page.py <morning.json> <out.html> [--other <file of the other mode>]

One place to look: what needs you on the left, the item you opened on the right. Rows of one line.
Marks: none for a fact Silo saw, ◌ a report the probe told, ◇ an opinion with whose, «» words an
agent wrote. A mark's meaning is in its tooltip. The reader's answer sits under the words it is
about and cites receipt lines by number; the cited lines carry a mark in the gutter.
"""
import html, json, sys

data = json.load(open(sys.argv[1]))
out = sys.argv[2]
other = sys.argv[sys.argv.index("--other") + 1] if "--other" in sys.argv else None
night = data["night"]

CSS = r"""
:root{--paper:#f7f7f4;--surface:#fafaf8;--surface-hover:#fff;--media:#eeefed;--ink:#282a2f;--muted:#666a73;--faint:#8a8e95;
--line:#dfe1df;--accent:#ff9f0a;--accent-ink:#995700;--accent-wash:#fff2dc;--no:#c4493c;--sans:"IBM Plex Sans",-apple-system,system-ui,Arial,sans-serif;
--serif:"IBM Plex Serif",Georgia,serif;--mono:"IBM Plex Mono",ui-monospace,SFMono-Regular,Menlo,monospace;--r:3px;--s1:4px;--s2:8px;--s3:12px;--s4:16px;--s5:24px}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--paper:#171a1b;--surface:#202425;--surface-hover:#282d2e;--media:#192122;--ink:#e5e7e4;
--muted:#a5adaa;--faint:#767d7b;--line:#343b3b;--accent-ink:#e8b66e;--accent-wash:#30291f;--no:#ee7d70}}
:root[data-theme="dark"]{--paper:#171a1b;--surface:#202425;--surface-hover:#282d2e;--media:#192122;--ink:#e5e7e4;--muted:#a5adaa;--faint:#767d7b;
--line:#343b3b;--accent-ink:#e8b66e;--accent-wash:#30291f;--no:#ee7d70}
*{box-sizing:border-box}
html,body{margin:0;background:var(--paper);color:var(--ink);font:14px/20px var(--sans)}
button{font:inherit;color:inherit;background:none;border:0;padding:0;cursor:pointer;text-align:left}
:focus-visible{outline:2px solid var(--accent);outline-offset:1px;border-radius:var(--r)}
header{display:flex;align-items:center;gap:var(--s3);padding:var(--s3) var(--s5);border-bottom:1px solid var(--line);min-height:49px}
header h1{font-size:15px;font-weight:600;margin:0}
header .sum{color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0;flex:1}
.menu{position:relative}
.menu>button{border:1px solid var(--line);border-radius:var(--r);padding:2px var(--s2);background:var(--surface);min-width:32px;text-align:center}
.menu>button:hover{background:var(--surface-hover)}
.menu ul{display:none;position:absolute;right:0;top:calc(100% + 4px);margin:0;padding:var(--s1) 0;list-style:none;background:var(--surface);
border:1px solid var(--line);border-radius:var(--r);min-width:230px;z-index:5}
.menu.open ul{display:block}
.menu li a,.menu li button{display:block;width:100%;padding:var(--s1) var(--s3);color:var(--ink);text-decoration:none}
.menu li a:hover,.menu li button:hover{background:var(--media)}
.menu li[aria-current] a{color:var(--accent-ink);font-weight:600}
main{display:grid;grid-template-columns:minmax(340px,5fr) 7fr;min-height:calc(100vh - 49px)}
.list{border-right:1px solid var(--line);padding:var(--s3) 0}
.list h2{font-size:12px;font-weight:600;color:var(--muted);margin:var(--s3) var(--s5) var(--s1);display:flex;gap:var(--s2)}
.list h2 .n{color:var(--faint);font-weight:400}
.row{display:grid;grid-template-columns:16px minmax(0,1fr) auto auto;gap:var(--s2);align-items:center;width:100%;padding:3px var(--s5);height:28px}
.row:hover{background:var(--surface-hover)}
.row[aria-selected="true"]{background:var(--accent-wash);box-shadow:inset 3px 0 var(--accent)}
.row .what{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.row .who,.row .at{color:var(--muted);white-space:nowrap;font-size:12px}
.row .at{font-variant-numeric:tabular-nums;width:38px;text-align:right}
.ic{width:16px;text-align:center;color:var(--faint);font-size:12px}
.ic.bad{color:var(--no)}
.empty{color:var(--muted);padding:3px var(--s5)}
.detail{padding:var(--s4) var(--s5) var(--s5);min-width:0}
.dh{display:flex;align-items:baseline;gap:var(--s3);flex-wrap:wrap;margin-bottom:var(--s3)}
.dh h3{font-size:16px;margin:0;font-weight:600}
.dh .meta{color:var(--muted)}
.dh .act{margin-left:auto}
.btn{border:1px solid var(--line);border-radius:var(--r);padding:2px var(--s3);background:var(--surface)}
.btn:hover{background:var(--surface-hover)}
.btn.primary{background:var(--accent);border-color:var(--accent);color:#282016}
.lines{display:grid;grid-template-columns:14px 34px 104px minmax(0,1fr);column-gap:var(--s2);font-size:13px}
.lines>div{display:contents}
.lines .g,.lines .n,.lines .sec,.lines .t{padding:2px 0;border-top:1px solid transparent}
.lines .n{color:var(--faint);font:12px/20px var(--mono);text-align:right}
.lines .sec{color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.lines .t{font:12.5px/20px var(--mono);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.lines .first>*{border-top-color:var(--line)}
.lines .hl>*{background:var(--accent-wash)}
.g .pin{display:inline-block;width:8px;height:8px;border-radius:50%;background:var(--no);margin-top:6px}
.g .pin.ok{background:var(--faint)}
.words{margin:var(--s3) 0;padding:var(--s2) var(--s3);border-left:3px solid var(--line);background:var(--surface)}
.words .said{font:italic 15px/22px var(--serif)}
.words .lbl{color:var(--muted);font-size:12px}
.ans{display:flex;gap:var(--s2);align-items:baseline;flex-wrap:wrap;margin-top:var(--s1);font-size:13px}
.ans .says{font-weight:600}
.ans .says.bad{color:var(--no)}
.ans .who{color:var(--muted);font-size:12px}
.cite{font:12px/18px var(--mono);border:1px solid var(--line);border-radius:var(--r);padding:0 4px;background:var(--paper)}
.cite:hover,.cite:focus-visible{border-color:var(--accent)}
.fact-mark{color:var(--no)}
q.w{font-family:var(--serif);font-style:italic;quotes:"«" "»"}
.rep{color:var(--faint);font-style:italic}
.op{color:var(--muted)}
.bad{color:var(--no)}
[data-tip]{position:relative}
[data-tip]:hover::after,[data-tip]:focus-visible::after{content:attr(data-tip);position:absolute;left:0;top:100%;z-index:9;white-space:nowrap;
background:var(--ink);color:var(--paper);font:12px/18px var(--sans);font-style:normal;padding:2px 6px;border-radius:var(--r);pointer-events:none}
.chain{position:relative;margin:0;padding:0 0 0 var(--s4);list-style:none;font-size:13px}
.chain::before{content:"";position:absolute;left:4px;top:8px;bottom:8px;border-left:1px solid var(--line)}
.chain li{position:relative;padding:2px 0;font:12.5px/20px var(--mono);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.chain li::before{content:"";position:absolute;left:-15px;top:8px;width:5px;height:5px;border-radius:50%;background:var(--faint)}
.chain li.sep{font:600 12px/20px var(--sans);color:var(--muted);margin-top:var(--s2)}
.chain li.sep::before{display:none}
.tbl{display:grid;grid-template-columns:minmax(0,1.2fr) 70px minmax(0,1fr) minmax(0,1.2fr);column-gap:var(--s3);font-size:13px}
.tbl>div{display:contents}.tbl span{padding:3px 0;border-top:1px solid var(--line);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.tbl .h span{color:var(--muted);font-size:12px;border-top:0}
.muted{color:var(--muted)}
.sub{margin:var(--s4) 0 var(--s1);font-size:12px;font-weight:600;color:var(--muted)}
@media (max-width:820px){main{grid-template-columns:1fr}.list{border-right:0;border-bottom:1px solid var(--line)}
header{padding:var(--s2) var(--s4)}.list h2,.row,.empty{padding-left:var(--s4);padding-right:var(--s4)}.detail{padding:var(--s4)}
.lines{grid-template-columns:12px 30px minmax(0,1fr)}.lines .sec{display:none}.row .who{display:none}}
"""

JS = r"""
const D = JSON.parse(document.getElementById('data').textContent);
const $ = (s, e = document) => e.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const TIP = {rep: 'Told by the probe, unverified', op: 'Opinion: Silo’s classifier', w: 'Words the agent wrote'};
function marks(t) {
  let s = esc(t);
  s = s.replace(/«(.*?)»/g, '<q class="w" data-tip="Words the agent wrote">$1</q>');
  s = s.replace(/◌([^·]*)/g, '<span class="rep" tabindex="0" data-tip="Told by the probe, unverified">◌$1</span>');
  s = s.replace(/◇/g, '<span class="op" tabindex="0" data-tip="Opinion: Silo’s classifier">◇</span>');
  s = s.replace(/(✗|⚠[^·]*)/g, '<span class="bad">$1</span>');
  return s;
}
function who(a) {
  if (!a) return '';
  if (a.run) return `<span class="op" tabindex="0" data-tip="${esc(short(a.how))}">◇ ${esc(a.run)}</span>`;
  if (a.probe) return `<span class="rep" tabindex="0" data-tip="Probe: its process did it">◌ ${esc(a.probe)}</span>`;
  return `<span class="muted" tabindex="0" data-tip="${esc(short(a.how || 'No author in the record'))}">no author</span>`;
}
function short(s) { const w = String(s || '').split(' '); return w.length <= 6 ? s : w.slice(0, 6).join(' ') + '…'; }
const SEV = {mismatch: 'bad', unbacked: 'bad'};
const SAYS = {mismatch: 'mismatch', unbacked: 'nothing in the record shows it', matched: 'the receipt shows what it says', 'not checked': 'not checked'};

function rowsFor() {
  const q = D.queue.map(x => {
    let ic = '!', what = '', bad = false;
    if (x.type === 'alert' && x.rule.startsWith('outside')) { ic = '−'; bad = true; what = x.text.replace(/: .*$/, ''); }
    else if (x.type === 'alert' && x.rule.startsWith('stuck')) { ic = '⏸'; what = 'Stuck: no model call, a command still runs'; }
    else if (x.type === 'alert' && x.rule.startsWith('agent-settings')) { ic = '✎'; what = x.text.replace(/changed /g, '').replace(/;/g, ',') + ' changed'; }
    else if (x.type === 'alert') { what = x.text; }
    else if (x.type === 'tls') { ic = '⊘'; bad = true; what = `Does not trust Silo’s certificate · ${x.host} · ${x.n} refusals`; }
    else if (x.type === 'words') { ic = '«'; bad = SEV[x.says] === 'bad'; what = `${x.claim ? '«' + x.claim + '» ' : ''}${x.speaks ? 'its words speak to the checker' : SAYS[x.says]}`; }
    else if (x.type === 'hosts') { ic = '↗'; what = `${x.text}: ${x.hosts.map(h => h.host + (h.decision === 'denied' ? ' (denied)' : '')).join(', ')}`; }
    const w = x.parts ? x.parts.map(p => who(p.author)).join(' ') : x.type === 'tls' && !x.author.run ? `<span class="muted">${esc(x.computer)}</span>` : who(x.author);
    return {id: x.id, ic, bad, what, who: w, at: x.at, open: x.opens, item: x};
  });
  return q;
}
function row(r) {
  return `<button class="row" role="option" aria-selected="false" data-id="${esc(r.id)}"><span class="ic${r.bad ? ' bad' : ''}" aria-hidden="true">${esc(r.ic)}</span>` +
    `<span class="what">${marks(r.what)}</span><span class="who">${r.who}</span><span class="at">${esc(r.at || '')}</span></button>`;
}
function list() {
  const q = rowsFor();
  const away = D.away.map((a, i) => ({id: 'away' + i, ic: a.text.startsWith('denied') ? '⊘' : '⏱', what: `${a.text} · ${a.answer}`, who: who(a.author), at: a.at, open: null}));
  const done = D.runs.filter(r => r.state === 'ended').map(r => ({id: 'run:' + r.name, ic: '✓', what: `${r.name} · ${r.computer} · ${dur(r.took_s)} · ${r.spend.replace(/^spend not known.*/, 'spend not known')} · ${r.files} files`, who: '', at: '', open: {receipt: r.name}}));
  const run = D.runs.filter(r => r.state !== 'ended').map(r => ({id: 'run:' + r.name, ic: '…', what: `${r.name} · ${r.computer} · ${r.tls ? 'waiting for the network' : r.state} for ${dur(r.for_s)}`, who: '', at: '', open: {receipt: r.name}}));
  const sec = (t, rs, empty) => `<h2>${t}<span class="n">${rs.length || ''}</span></h2>` + (rs.length ? rs.map(row).join('') : `<div class="empty">${empty}</div>`);
  ROWS = Object.fromEntries([...q, ...away, ...done, ...run].map(r => [r.id, r]));
  $('.list').innerHTML = sec('Needs you', q, 'Nothing needs you.') + (away.length ? sec('Silo answered', away) : '') + sec('Done', done, 'No run has ended.') + (run.length ? sec('Still running', run) : '');
}
function dur(s) { s = Math.round(s || 0); return s < 60 ? s + ' s' : s < 3600 ? Math.floor(s / 60) + ' min' : Math.floor(s / 3600) + ' h ' + String(Math.floor(s % 3600 / 60)).padStart(2, '0'); }
let ROWS = {};
function open(id) {
  document.querySelectorAll('.row').forEach(b => b.setAttribute('aria-selected', b.dataset.id === id));
  const r = ROWS[id]; if (!r) return;
  const o = r.open || {};
  $('.detail').innerHTML = o.receipt ? receipt(o.receipt, r.item) : o.chain ? chain(o.chain, r.item) : o.hosts ? hosts(r.item) : `<div class="dh"><h3>${marks(r.what)}</h3></div>`;
  $('.detail').querySelectorAll('.cite').forEach(c => {
    const on = v => c.dataset.n.split(' ').forEach(n => { const l = $(`.lines [data-n="${n}"]`); if (l) l.classList.toggle('hl', v); });
    c.addEventListener('mouseenter', () => on(true)); c.addEventListener('mouseleave', () => on(false));
    c.addEventListener('focus', () => on(true)); c.addEventListener('blur', () => on(false));
    c.addEventListener('click', () => { const l = $(`.lines [data-n="${c.dataset.n.split(' ')[0]}"]`); if (l) l.firstElementChild.scrollIntoView({block: 'center', behavior: 'smooth'}); });
  });
  try { localStorage.setItem('morning-open', id); } catch (e) {}
}
function linesHtml(ls, cited) {
  let prev = null;
  return '<div class="lines">' + ls.map(l => {
    const c = cited[l.n];
    const g = c ? `<span class="pin${c === 'bad' ? '' : ' ok'}" tabindex="0" data-tip="Cited by the reader’s answer"></span>` : '';
    const first = l.section !== prev; prev = l.section;
    return `<div class="${first ? 'first' : ''}" data-n="${l.n}"><span class="g">${g}</span><span class="n">${l.n}</span><span class="sec">${first ? esc(l.section) : ''}</span><span class="t" title="${esc(l.text)}">${marks(l.text)}</span></div>`;
  }).join('') + '</div>';
}
function receipt(name, item) {
  const R = D.receipts[name];
  const run = D.runs.find(r => r.name === name);
  const cited = {};
  for (const a of R.answers) for (const n of a.rests_on || []) if (!cited[n] || SEV[a.says]) cited[n] = SEV[a.says] || 'ok';
  const outcome = R.lines.filter(l => l.section === 'Outcome' || l.section === 'TLS');
  const rest = R.lines.filter(l => !(l.section === 'Outcome' || l.section === 'TLS'));
  let words = '';
  if (R.words) {
    const top = R.answers[0];
    const speaks = R.speaks_to_checker || R.shaped;
    let ans = '';
    if (R.shaped) ans += `<div class="ans"><span class="fact-mark">its words speak to the checker:</span> <q class="w">${esc(R.shaped)}</q></div>`;
    else if (R.speaks_to_checker) ans += `<div class="ans"><span class="op">◇</span> <span class="fact-mark">its words speak to the checker</span> <span class="who">claim-list-reader@1</span></div>`;
    if (top) {
      const says = top.says === 'matched' && speaks ? 'matched, but its words speak to the checker' : SAYS[top.says] || 'not read: ' + (top.why || 'no answer');
      const cites = (top.rests_on || []).length ? `rests on ${top.rests_on.map(n => `<button class="cite" data-n="${n}" aria-label="Receipt line ${n}">${n}</button>`).join(' ')}` : `<span class="fact-mark">rests on no line of the receipt</span>`;
      const both = R.answers.map(a => `${esc(a.reader)}: ${esc(a.says || 'no answer')}`).join(' · ');
      ans += `<div class="ans"><span class="op" tabindex="0" data-tip="Opinion of two model readers">◇</span><span class="says ${SEV[top.says] || speaks ? 'bad' : ''}">${esc(says)}</span>` +
        `${top.claim ? `<q class="w">${esc(top.claim)}</q>` : ''}<span>${cites}</span><span class="who" title="${esc(both)}">${esc(R.answers.map(a => a.reader).join(' + '))}</span></div>`;
    }
    words = `<div class="words"><div class="lbl">Its words · ${esc(R.words_at || '')}</div><div class="said">«${esc(R.words)}»</div>${ans}</div>`;
  } else {
    words = `<div class="words"><div class="lbl">Its words</div><div class="muted">${run && run.tls ? 'none: it never reached its model' : 'none yet: the run is not finished'}</div></div>`;
  }
  const asked = R.asked ? `<div class="sub">You asked · ${esc(R.asked_at || '')}</div><div><q class="w" data-tip="Words you wrote">${esc(R.asked)}</q></div>` : '';
  return `<div class="dh"><h3>${esc(name)}</h3><span class="meta">${esc(R.head[0].replace(/^RECEIPT\s+\S+ #\d+ · /, ''))}</span><span class="meta">${marks(R.head[1])}</span></div>` +
    linesHtml(outcome, cited) + asked + words + `<div class="sub">Receipt</div>` + linesHtml(rest, cited);
}
function chain(id, item) {
  const ls = D.chains[id] || [];
  const put = item && item.pins ? `<button class="btn primary act" data-tip="From checkpoint ${esc(item.pins)}">Put back</button>` : '';
  const a = item && item.author;
  const by = a && a.run ? `<div class="ans"><span class="op" tabindex="0" data-tip="Opinion: author from traffic">◇</span> ${esc(a.run)} · ${esc(a.how)}${a.tool_call ? ` · <code>${esc(a.tool_call)}</code>` : ''}</div>` : '';
  return `<div class="dh"><h3>${marks(item ? item.text.replace(/;/g, ',') : id)}</h3><span class="meta">${esc(item ? item.computer + ' · ' + item.at + ' · your ' + item.rule_kind + ' ' + item.rule : '')}</span>${put}</div>` + by +
    `<div class="sub">What led to it</div><ul class="chain">` + ls.filter(l => !/^ ── what led/.test(l)).map(l => /^ ──/.test(l) ? `<li class="sep">${esc(l.replace(/─/g, '').trim())}</li>` : `<li title="${esc(l.trim())}">${marks(l.trim())}</li>`).join('') + '</ul>';
}
function hosts(item) {
  const hs = D.hosts;
  const rows = hs.map(h => `<div><span>${esc(h.host)}${h.start_host_of ? ` <span class="muted" tabindex="0" data-tip="Reached by ${esc(h.start_host_of)} at start">· at start</span>` : ''}</span><span>${esc(h.computer)}</span><span class="${h.decision === 'denied' || /refused/.test(h.decision) ? 'bad' : ''}">${esc(h.decision)}</span><span>${who(h.author)}</span></div>`).join('');
  const t = D.queue.find(q => q.type === 'tls');
  const tls = t && !t.author.run ? `<div class="sub">Refused Silo’s certificate</div><div>${esc(t.n)} handshakes on ${esc(t.host)} · alert ${esc(t.alert)} · since ${esc(t.at)} · ${esc(t.author.how)}</div>` : '';
  return `<div class="dh"><h3>New hosts</h3><span class="meta">${hs.filter(h => !h.start_host_of).length} new, ${hs.filter(h => h.start_host_of).length} reached by a program at its start</span></div>` +
    `<div class="tbl"><div class="h"><span>Host</span><span>Computer</span><span>Silo</span><span>Who</span></div>${rows}</div>` + tls;
}
list();
document.querySelector('.list').addEventListener('click', e => { const b = e.target.closest('.row'); if (b) open(b.dataset.id); });
document.querySelector('.list').addEventListener('keydown', e => {
  if (!['ArrowDown', 'ArrowUp'].includes(e.key)) return;
  const bs = [...document.querySelectorAll('.row')]; const i = bs.indexOf(document.activeElement);
  const n = bs[Math.max(0, Math.min(bs.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))]; if (n) { e.preventDefault(); n.focus(); open(n.dataset.id); }
});
const m = $('.menu'); $('.menu>button').addEventListener('click', () => { m.classList.toggle('open'); $('.menu>button').setAttribute('aria-expanded', m.classList.contains('open')); });
document.addEventListener('click', e => { if (!m.contains(e.target)) m.classList.remove('open'); });
$('#theme').addEventListener('click', () => { const r = document.documentElement; const d = r.dataset.theme === 'dark' || (!r.dataset.theme && matchMedia('(prefers-color-scheme: dark)').matches); r.dataset.theme = d ? 'light' : 'dark'; try { localStorage.setItem('morning-theme', r.dataset.theme); } catch (e) {} });
try { const t = localStorage.getItem('morning-theme'); if (t) document.documentElement.dataset.theme = t; } catch (e) {}
let first = null; try { first = new URLSearchParams(location.search).get('open') || localStorage.getItem('morning-open'); } catch (e) {}
open(ROWS[first] ? first : Object.keys(ROWS)[0]);
"""

mode_probe = night["mode"] == "probe"
unknown = (" + " + ", ".join(f"{u}: not known" for u in night["unknown"])) if night["unknown"] else ""
summary = f"{night['left_at']} → {night['until']} · {night['runs']} runs · {night['turns']} turns · {night['spend']}{unknown}"
here_name = out.rsplit("/", 1)[-1]
items = []
if other:
    a, b = (here_name, other) if mode_probe else (other, here_name)
    items.append(f'<li{" aria-current" if mode_probe else ""}><a href="{html.escape(a)}">With the probe</a></li>')
    items.append(f'<li{"" if mode_probe else " aria-current"}><a href="{html.escape(b)}">Traffic only (option A)</a></li>')
items.append('<li><button id="theme">Light or dark</button></li>')
doc = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Silo morning</title>
<style>{CSS}</style></head><body>
<header><h1>Morning</h1><span class="sum">{html.escape(summary)}</span>
<div class="menu"><button aria-haspopup="true" aria-expanded="false" aria-label="Menu">⋯</button><ul>{''.join(items)}</ul></div></header>
<main><nav class="list" role="listbox" aria-label="What needs you"></nav><section class="detail" aria-live="polite"></section></main>
<script type="application/json" id="data">{json.dumps(data, ensure_ascii=False).replace("</", "<\\/")}</script>
<script>{JS}</script>
</body></html>"""
open(out, "w").write(doc)
print(out)
