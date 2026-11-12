// RustMotion — horloge virtuelle injectée avant tout script de la page.
// Rend la page déterministe : le temps n'avance que lorsque le moteur appelle __rm.seek(t).
(() => {
  if (window.__rm) return;
  const W = window;
  const realRAF = W.requestAnimationFrame.bind(W);
  const realSetTimeout = W.setTimeout.bind(W);
  const RealDate = W.Date;
  const EPOCH = RealDate.now();
  let now = 0; // temps virtuel (ms)

  // --- performance.now / Date -------------------------------------------------
  try { performance.now = () => now; } catch (e) {}
  function FakeDate(...args) {
    if (!new.target) return new RealDate(EPOCH + now).toString();
    return args.length ? new RealDate(...args) : new RealDate(EPOCH + now);
  }
  FakeDate.prototype = RealDate.prototype;
  FakeDate.now = () => EPOCH + now;
  FakeDate.parse = RealDate.parse;
  FakeDate.UTC = RealDate.UTC;
  W.Date = FakeDate;

  // --- requestAnimationFrame --------------------------------------------------
  let rafSeq = 0;
  let rafCbs = new Map();
  W.requestAnimationFrame = (cb) => { const id = ++rafSeq; rafCbs.set(id, cb); return id; };
  W.cancelAnimationFrame = (id) => { rafCbs.delete(id); };

  // --- setTimeout / setInterval -----------------------------------------------
  let timerSeq = 0;
  const timers = new Map();
  const toFn = (fn) => (typeof fn === 'function' ? fn : new Function(String(fn)));
  W.setTimeout = (fn, delay, ...args) => {
    const id = ++timerSeq;
    timers.set(id, { fn: toFn(fn), args, at: now + Math.max(0, Number(delay) || 0), every: 0 });
    return id;
  };
  W.setInterval = (fn, delay, ...args) => {
    const every = Math.max(1, Number(delay) || 0);
    const id = ++timerSeq;
    timers.set(id, { fn: toFn(fn), args, at: now + every, every });
    return id;
  };
  W.clearTimeout = W.clearInterval = (id) => { timers.delete(id); };

  // --- Animations CSS / Web Animations ---------------------------------------
  // Chaque animation reçoit un instant de départ virtuel lorsqu'on la découvre.
  const starts = new WeakMap();
  let freezeByPause = false; // repli si la timeline du document ne peut pas être gelée
  const scan = () => {
    let list;
    try { list = document.getAnimations(); } catch (e) { return []; }
    for (const a of list) {
      if (!starts.has(a)) {
        starts.set(a, now);
        if (freezeByPause) { try { a.pause(); } catch (e) {} }
      }
    }
    return list;
  };

  const syncAnimations = (t) => {
    for (const a of scan()) {
      const v = starts.get(a);
      if (v === undefined) continue;
      // Une animation mise en pause par la page elle-même reste figée.
      if (!freezeByPause && a.playState === 'paused') continue;
      try {
        const rate = a.playbackRate || 1;
        let ct = Math.max(0, t - v) * Math.abs(rate);
        if (rate < 0) {
          const end = a.effect ? a.effect.getComputedTiming().endTime : 0;
          ct = Math.max(0, end - ct);
        }
        a.currentTime = ct;
      } catch (e) {}
    }
  };

  // --- Médias -----------------------------------------------------------------
  const syncMedia = async (t) => {
    const waits = [];
    for (const m of document.querySelectorAll('video, audio')) {
      try {
        if (!m.paused) m.pause();
        if (m.readyState < 1 || !isFinite(m.duration) || m.duration <= 0) continue;
        let target = t / 1000;
        target = m.loop ? target % m.duration : Math.min(target, m.duration);
        if (Math.abs(m.currentTime - target) > 0.0005) {
          waits.push(new Promise((r) => {
            const done = () => { m.removeEventListener('seeked', done); r(); };
            m.addEventListener('seeked', done);
            realSetTimeout(done, 1000);
          }));
          m.currentTime = target;
        }
      } catch (e) {}
    }
    if (waits.length) await Promise.all(waits);
  };

  const nextPaint = () => new Promise((r) => {
    let done = false;
    const fin = () => { if (!done) { done = true; r(); } };
    realRAF(fin);
    realSetTimeout(fin, 100);
  });

  const runTimersUntil = (t) => {
    for (let guard = 0; guard < 200000; guard++) {
      let nextId = -1, next = null;
      for (const [id, tm] of timers) {
        if (tm.at <= t && (next === null || tm.at < next.at)) { next = tm; nextId = id; }
      }
      if (next === null) return;
      now = Math.max(now, next.at);
      if (next.every) next.at += next.every; else timers.delete(nextId);
      try { next.fn.apply(W, next.args); } catch (e) { console.error(e); }
      scan();
    }
  };

  let lastT = -1;
  W.__rm = {
    version: 1,
    get now() { return now; },
    // Délai en temps réel (non virtualisé).
    sleep(ms) { return new Promise((r) => realSetTimeout(r, ms)); },
    // Appelé une fois la page chargée. `timelineFrozen` : la timeline a-t-elle été gelée via CDP ?
    async init(timelineFrozen) {
      if (!timelineFrozen) {
        const t0 = document.timeline ? document.timeline.currentTime : null;
        await new Promise((r) => realSetTimeout(r, 60));
        const t1 = document.timeline ? document.timeline.currentTime : null;
        freezeByPause = t0 === null || t1 === null || t1 !== t0;
      }
      scan();
      return { freezeByPause };
    },
    async seek(t) {
      if (t < lastT) throw new Error('RustMotion : le temps ne peut pas reculer');
      lastT = t;
      runTimersUntil(t);
      now = t;
      const cbs = rafCbs;
      rafCbs = new Map();
      for (const cb of cbs.values()) {
        try { cb(now); } catch (e) { console.error(e); }
      }
      await Promise.resolve();
      syncAnimations(t);
      await syncMedia(t);
      if (W.__rmWaitPaint !== false) await nextPaint();
      return now;
    },
    probe() {
      const out = { title: document.title || '', animations: 0, infinite: false, maxEnd: 0, gsap: false, gsapDuration: null, meta: {} };
      for (const a of scan()) {
        out.animations++;
        try {
          const ct = a.effect.getComputedTiming();
          if (!isFinite(ct.endTime)) out.infinite = true;
          else out.maxEnd = Math.max(out.maxEnd, ct.endTime);
        } catch (e) {}
      }
      const g = W.gsap;
      if (g && g.globalTimeline) {
        out.gsap = true;
        try {
          const d = g.globalTimeline.duration();
          if (isFinite(d) && d > 0 && d < 3600) out.gsapDuration = d; else if (d >= 3600) out.infinite = true;
        } catch (e) {}
      }
      const meta = (n) => {
        const el = document.querySelector(`meta[name="rustmotion:${n}"]`);
        return el ? Number(el.getAttribute('content')) : null;
      };
      const cfg = (W.RUSTMOTION && typeof W.RUSTMOTION === 'object') ? W.RUSTMOTION : {};
      for (const k of ['duration', 'width', 'height', 'fps']) {
        const v = cfg[k] != null ? Number(cfg[k]) : meta(k);
        out.meta[k] = (v != null && isFinite(v) && v > 0) ? v : null;
      }
      return out;
    },
  };
})();
