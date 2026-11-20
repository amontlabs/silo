/* The voice layer: narration over a Fjord film.
   It reads window.FILM_VOICE_DATA (voice.json with each clip inlined as a data URL) and gives the kit window.FILM_VOICE:
   - V.at(scene, sentence): seconds into the scene where that sentence starts, for a motion's numeric `at`;
     V.word(scene, sentence, "phrase") where a phrase of it is said (estimated); V.end(scene, sentence) where it ends;
     V.total(scene) the scene's narration with its silences.
   - V.frame(tl, starts, scene): called by the player on every frame. It shows the sentence being spoken under the
     stage, and keeps the sound on the timeline: the timeline is the clock, and each clip is started at the offset the
     timeline asks for, then restarted whenever the two part by more than a tenth of a second. Every sentence starts
     from the timeline again, so nothing drifts over a film.
   Without a click the browser keeps sound off, so with a voice the film opens paused behind a Play button.
   Headless, nothing here waits for sound: no clip is decoded before the first click. */
(() => {
  const data = window.FILM_VOICE_DATA;
  if (!data || !data.voices || !data.voices.length) return;
  const q = new URLSearchParams(location.search);
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* the film works without it */ } },
  };
  let vi = data.voices.findIndex(v => v.id === (q.get("voice") || store.get("film.voice")));
  if (vi < 0) vi = 0;
  let muted = store.get("film.muted") === "1";
  const voice = () => data.voices[vi];
  const SYNC = 0.1;          // seconds the sound may part from the timeline before it is restarted
  const TAIL = data.between ?? 0.35; // a subtitle stays through the silence after its sentence

  const sentence = (s, j, fn) => {
    const sc = voice().scenes[s];
    if (!sc) throw new Error(`V.${fn}(${s}, ${j}): the narration has no scene ${s} (scenes count from 0).`);
    const x = sc.sentences[j];
    if (!x) throw new Error(`V.${fn}(${s}, ${j}): scene ${s} has ${sc.sentences.length} sentences (they count from 0).`);
    return x;
  };

  // ---------- sound ----------
  let ctx = null, gain = null, playing = null; // playing: { key, node, t0, off0 }
  const buffers = new Map(), decoding = new Map();
  const keyOf = (v, s, j) => `${v}/${s}/${j}`;
  function bytes(url) {
    const b64 = url.slice(url.indexOf(",") + 1), bin = atob(b64), out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out.buffer;
  }
  function load(v, s, j) {
    const k = keyOf(v, s, j);
    if (buffers.has(k) || decoding.has(k) || !ctx) return;
    const src = data.voices[v].scenes[s].sentences[j].src;
    const p = (src.startsWith("data:") ? Promise.resolve(bytes(src)) : fetch(src).then(r => r.arrayBuffer()))
      .then(ab => ctx.decodeAudioData(ab))
      .then(b => { buffers.set(k, b); decoding.delete(k); })
      .catch(e => { decoding.delete(k); console.error("voice: could not decode", src.slice(0, 40), e); });
    decoding.set(k, p);
  }
  function loadVoice(v) { data.voices[v].scenes.forEach((sc, s) => sc.sentences.forEach((_, j) => load(v, s, j))); }
  function unlock() {
    if (!ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) return;
      ctx = new AC();
      gain = ctx.createGain();
      gain.connect(ctx.destination);
      gsap.ticker.lagSmoothing(0); // the timeline follows real time, as the sound does
      loadVoice(vi);
    }
    if (ctx.state === "suspended") ctx.resume();
  }
  function stop() {
    if (!playing) return;
    try { playing.node.stop(); } catch (e) { /* already ended */ }
    playing.node.disconnect();
    playing = null;
  }
  function sound(tl, scene, local, x, j) {
    const on = ctx && !muted && !tl.paused() && tl.time() < tl.duration() - 0.01 && x && local < x.start + x.length;
    if (!on) return stop();
    const k = keyOf(vi, scene, j), want = local - x.start;
    if (playing && playing.key === k && Math.abs(playing.off0 + (ctx.currentTime - playing.t0) - want) < SYNC) return;
    stop();
    const b = buffers.get(k);
    if (!b) return load(vi, scene, j);
    if (want >= b.duration - 0.02) return;
    const node = ctx.createBufferSource();
    node.buffer = b;
    node.connect(gain);
    node.start(0, Math.max(0, want));
    playing = { key: k, node, t0: ctx.currentTime, off0: Math.max(0, want) };
  }

  // ---------- subtitles and controls ----------
  const $ = s => document.querySelector(s);
  let said = null;
  function frame(tl, starts, scene) {
    const sc = voice().scenes[scene];
    const local = tl.time() - (starts[scene] || 0);
    let j = -1;
    if (sc) sc.sentences.forEach((x, n) => { if (local >= x.start - 0.001) j = n; });
    const x = j >= 0 ? sc.sentences[j] : null;
    const shown = x && local < x.start + x.length + TAIL ? x.text : "";
    const sub = $(".subtitle");
    if (sub && said !== shown) { sub.textContent = shown; said = shown; }
    sound(tl, scene, local, x, j);
    const big = $(".bigplay");
    if (big && !tl.paused()) big.hidden = true;
  }

  // Where the film stands as (scene, sentence, seconds into it), so a voice switch lands on the same words.
  function place(t, starts) {
    let s = 0;
    starts.forEach((a, i) => { if (t >= a - 0.001) s = i; });
    const local = t - starts[s], sc = voice().scenes[s];
    let j = -1;
    sc.sentences.forEach((x, n) => { if (local >= x.start - 0.001) j = n; });
    return { s, j, into: j >= 0 ? local - sc.sentences[j].start : local };
  }
  function switchVoice(nv) {
    const p = window.__player, film = window.__film && window.__film.timeline;
    if (!p || !film || nv === vi) return;
    const at = place(film.time(), p.starts);
    stop();
    vi = nv;
    store.set("film.voice", voice().id);
    if (ctx) loadVoice(vi);
    p.rebuild();
    const sc = voice().scenes[at.s], starts = p.starts;
    const x = at.j >= 0 ? sc.sentences[at.j] : null;
    p.seek(starts[at.s] + (x ? x.start + Math.min(at.into, x.length) : Math.min(at.into, sc.sentences[0] ? sc.sentences[0].start : at.into)));
  }

  function controls() {
    const nav = $("nav"), pp = $(".pp");
    if (!nav) return;
    const mute = document.createElement("button");
    mute.className = "mute";
    const paint = () => {
      mute.setAttribute("aria-pressed", muted);
      mute.textContent = muted ? "Sound off" : "Sound on";
      mute.title = muted ? "Turn the voice on" : "Turn the voice off; the subtitles go on";
    };
    paint();
    mute.onclick = () => { muted = !muted; store.set("film.muted", muted ? "1" : "0"); paint(); unlock(); if (muted) stop(); };
    nav.appendChild(mute);
    if (data.voices.length > 1) {
      const sel = document.createElement("select");
      sel.className = "voice";
      sel.setAttribute("aria-label", "Voice");
      data.voices.forEach((v, n) => { const o = document.createElement("option"); o.value = n; o.textContent = v.label; sel.appendChild(o); });
      sel.value = vi;
      sel.onchange = () => { unlock(); switchVoice(+sel.value); };
      nav.appendChild(sel);
    }
    // Any press may be the first: sound can start from it.
    ["pointerdown", "keydown"].forEach(e => addEventListener(e, unlock, true));
    // The film opens paused: one obvious Play over the stage, unless a frame was asked for (stills) or it is embedded.
    const big = $(".bigplay");
    if (big) {
      big.hidden = q.has("scene") || q.has("t") || q.has("embed");
      big.onclick = () => { unlock(); big.hidden = true; window.__player && window.__player.play(); };
    }
    if (pp) pp.addEventListener("click", unlock);
  }
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", controls); else controls();

  window.FILM_VOICE = {
    at: (s, j) => sentence(s, j, "at").start,
    end: (s, j) => { const x = sentence(s, j, "end"); return x.start + x.length; },
    // Where a phrase of the sentence is said, estimated from its place in the text (good to a few tenths of a second).
    word: (s, j, phrase) => {
      const x = sentence(s, j, "word"), k = x.text.indexOf(phrase);
      if (k < 0) throw new Error(`V.word(${s}, ${j}, "${phrase}"): the sentence is "${x.text}".`);
      return x.start + x.length * k / x.text.length;
    },
    total: s => (voice().scenes[s] ? voice().scenes[s].total : 0),
    count: s => (voice().scenes[s] ? voice().scenes[s].sentences.length : 0),
    get id() { return voice().id; },
    get muted() { return muted; },
    frame,
    // For tests: what is sounding and how far the sound stands from where the timeline wants it.
    debug: () => ({ ctx: ctx ? ctx.state : "none", decoded: buffers.size, playing: playing ? playing.key : null,
                    off: playing ? +(playing.off0 + (ctx.currentTime - playing.t0)).toFixed(3) : null,
                    subtitle: said, voice: voice().id, muted }),
  };
})();
