// Development builds only: measures the guest desktop stream from inside the
// viewer page and posts the report to `/__silo/v1/diagnostics`. Rust evaluates
// this with `(nonce, config)`. Latency probes need the guest fixture
// `scripts/desktop-stream-bench/responder.py` covering the screen: `a` swaps its
// fill between a red and a blue noise tile, `m` toggles a full-screen scroll.
(async (nonce, config) => {
  const started = performance.now()
  const report = { startedAt: new Date().toISOString(), config, errors: [] }
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))
  const post = async body => {
    const query = `nonce=${encodeURIComponent(nonce)}&kind=application/json`
    await fetch(`/__silo/v1/diagnostics?${query}`, { method: "POST", body: JSON.stringify(body), cache: "no-store", credentials: "same-origin" })
  }
  const describe = element => {
    const box = element.getBoundingClientRect()
    const style = getComputedStyle(element)
    return {
      tag: element.tagName.toLowerCase(),
      id: element.id || null,
      cssWidth: box.width,
      cssHeight: box.height,
      left: box.left,
      top: box.top,
      backingWidth: element.tagName === "VIDEO" ? element.videoWidth : element.width,
      backingHeight: element.tagName === "VIDEO" ? element.videoHeight : element.height,
      visible: box.width > 0 && box.height > 0 && style.display !== "none" && style.visibility !== "hidden" && Number(style.opacity) > 0,
      imageRendering: style.imageRendering,
      transform: style.transform,
    }
  }
  const sinks = () => [...document.querySelectorAll("video, canvas")].map(element => ({ element, info: describe(element) }))
  const send = message => {
    const transport = window.selkiesTransport
    if (!transport || typeof transport.send !== "function") throw new Error("no Selkies transport")
    transport.send(message)
  }
  const key = keysym => { send(`kd,${keysym}`); send(`ku,${keysym}`) }
  const summary = values => {
    if (!values.length) return null
    const sorted = [...values].sort((a, b) => a - b)
    const at = p => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]
    return {
      count: values.length,
      min: sorted[0],
      p50: at(0.5),
      p90: at(0.9),
      max: sorted[sorted.length - 1],
      mean: values.reduce((a, b) => a + b, 0) / values.length,
    }
  }
  // Absolute milliseconds, comparable between the page and a worker.
  const clock = () => performance.timeOrigin + performance.now()
  const classify = ([r, , b]) => r > b + 60 ? "red" : b > r + 60 ? "blue" : "other"

  // Calls `seen(pixel, t)` for every frame the sink receives. WebKit's drawImage
  // of a track-generator <video> returns a stale frame, so a <video> sink is
  // read from a clone of its track in a worker; a canvas sink is polled.
  const watch = (sink, seen) => {
    if (sink.info.tag === "video" && sink.element.srcObject) {
      const track = sink.element.srcObject.getVideoTracks()[0].clone()
      const code = `onmessage = async event => {
        const reader = new MediaStreamTrackProcessor({ track: event.data }).readable.getReader()
        const context = new OffscreenCanvas(4, 4).getContext("2d", { willReadFrequently: true })
        for (;;) {
          const { value, done } = await reader.read()
          if (done) return
          const t = performance.timeOrigin + performance.now()
          context.drawImage(value, value.displayWidth / 2 - 2, value.displayHeight / 2 - 2, 4, 4, 0, 0, 4, 4)
          value.close()
          postMessage({ t, pixel: [...context.getImageData(0, 0, 1, 1).data] })
        }
      }`
      const worker = new Worker(URL.createObjectURL(new Blob([code], { type: "text/javascript" })))
      worker.onmessage = event => seen(event.data.pixel, event.data.t)
      worker.onerror = event => report.errors.push(`frame watcher: ${event.message}`)
      worker.postMessage(track, [track])
      report.watcher = "track processor in a worker"
      return () => { worker.terminate(); track.stop() }
    }
    const context = Object.assign(document.createElement("canvas"), { width: 4, height: 4 })
      .getContext("2d", { willReadFrequently: true })
    let polling = true
    const poll = () => {
      if (!polling) return
      try {
        const element = sink.element
        context.drawImage(element, element.width / 2 - 2, element.height / 2 - 2, 4, 4, 0, 0, 4, 4)
        seen([...context.getImageData(0, 0, 1, 1).data], clock())
      } catch (error) {
        report.errors.push(`sink unreadable: ${error}`)
        return
      }
      setTimeout(poll, 1)
    }
    poll()
    report.watcher = "canvas polling"
    return () => { polling = false }
  }

  try {
    report.page = {
      devicePixelRatio: window.devicePixelRatio,
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      userAgent: navigator.userAgent,
      webCodecs: typeof VideoDecoder !== "undefined",
      hidden: document.hidden,
    }
    // Selkies samples only while a dashboard has its stats open.
    window.postMessage({ type: "statsOpen", open: true }, location.origin)
    await sleep(1500)

    const visible = sinks().filter(s => s.info.visible && s.info.backingWidth > 0)
    const sink = visible.find(s => s.info.tag === "video") || visible[0]
    report.sink = sink ? sink.info : null
    let latest = null
    const waiters = new Set()
    const stop = sink ? watch(sink, (pixel, t) => {
      latest = { cls: classify(pixel), t, pixel }
      for (const waiter of [...waiters]) waiter(latest)
    }) : () => {}
    const next = (accept, timeout) => new Promise(resolve => {
      const timer = setTimeout(() => { waiters.delete(waiter); resolve(null) }, timeout)
      const waiter = frame => {
        if (!accept(frame)) return
        waiters.delete(waiter)
        clearTimeout(timer)
        resolve(frame)
      }
      waiters.add(waiter)
    })
    // A damage-gated stream sends nothing while the screen is still, so the
    // first frame may need a change.
    if (!await next(() => true, 1500) && config.probes > 0) {
      key(97)
      await next(() => true, 3000)
    }
    report.sinkSample = latest && latest.pixel

    // Input-to-picture latency: a keypress until a frame with the swapped fill reaches the sink.
    const probes = []
    report.probeStarts = []
    if (latest && latest.cls === "other") report.errors.push("responder not visible: centre pixel is neither red nor blue")
    for (let i = 0; latest && latest.cls !== "other" && i < config.probes; i++) {
      await sleep(150 + Math.random() * 150)
      const before = latest.cls
      const t0 = clock()
      report.probeStarts.push(t0)
      key(97)
      const frame = await next(f => f.t >= t0 && f.cls !== before && f.cls !== "other", 3000)
      if (frame) probes.push(frame.t - t0)
      else report.errors.push(`probe ${i} timed out`)
    }
    stop()
    report.inputToPicture = summary(probes)
    report.inputToPictureSamples = probes.map(v => Math.round(v * 10) / 10)

    // Steady-state stream figures, idle then under a full-screen scroll.
    const phase = async (name, seconds) => {
      const history = window.stream_stats && window.stream_stats.history
      const from = history ? history.length : 0
      // rAF stops while the page is hidden, so it only counts frames here.
      let frames = 0
      let counting = true
      const count = () => { if (counting) { frames++; requestAnimationFrame(count) } }
      requestAnimationFrame(count)
      await sleep(seconds * 1000)
      counting = false
      const after = window.stream_stats && window.stream_stats.history
      return { name, seconds, rafPerSecond: frames / seconds, samples: after ? after.slice(from) : [] }
    }
    report.phases = []
    if (config.idleSeconds > 0) report.phases.push(await phase("idle", config.idleSeconds))
    if (config.motionSeconds > 0) {
      key(109)
      await sleep(500)
      report.phases.push(await phase("motion", config.motionSeconds))
      key(109)
    }
    report.streamInfo = window.stream_info || null
    report.streamClient = window.stream_client || null
    report.sinks = sinks().map(s => s.info)
    report.fps = window.fps
  } catch (error) {
    report.errors.push(String(error && error.stack || error))
  }
  report.elapsedMs = performance.now() - started
  try { window.postMessage({ type: "statsOpen", open: false }, location.origin) } catch { /* page gone */ }
  await post(report)
})
