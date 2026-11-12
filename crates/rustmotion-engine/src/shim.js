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
  // Compteurs d'activité : servent à reconnaître une page animée (voir probe()).
  const activity = { delayedTimeouts: 0, intervals: 0, animations: 0 };
  W.setTimeout = (fn, delay, ...args) => {
    const id = ++timerSeq;
    const d = Math.max(0, Number(delay) || 0);
    if (d >= 16) activity.delayedTimeouts++;
    timers.set(id, { fn: toFn(fn), args, at: now + d, every: 0 });
    return id;
  };
  W.setInterval = (fn, delay, ...args) => {
    activity.intervals++;
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
        activity.animations++;
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

  // Avance l'horloge à `t` : timers échus, puis une image requestAnimationFrame.
  const MAX_STEP = 50;
  const tick = async (t) => {
    runTimersUntil(t);
    now = t;
    const cbs = rafCbs;
    rafCbs = new Map();
    for (const cb of cbs.values()) {
      try { cb(now); } catch (e) { console.error(e); }
    }
    await Promise.resolve();
  };

  // Protocole « seek » : une page qui expose window.seek(t) (t en secondes) avec window.DURATION,
  // ou window.RUSTMOTION.seek(t), dessine elle-même l'image à l'instant t ; on l'appelle à chaque image.
  const pageSeek = () => {
    const cfg = W.RUSTMOTION;
    if (cfg && typeof cfg.seek === 'function') return cfg.seek;
    if (typeof W.seek === 'function' && typeof W.DURATION === 'number') return W.seek;
    return null;
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
      if (t < lastT) throw new Error('RustMotion: time cannot go backwards');
      lastT = t;
      // Grand saut (miniature, décalage de départ) : on avance par petits pas, sinon les
      // bibliothèques qui lissent les à-coups (lagSmoothing de GSAP…) ne suivraient pas.
      while (t - now > MAX_STEP) await tick(now + MAX_STEP);
      await tick(t);
      const draw = pageSeek();
      if (draw) {
        try { await draw.call(W, t / 1000); } catch (e) { console.error(e); }
      }
      syncAnimations(t);
      await syncMedia(t);
      if (W.__rmWaitPaint !== false) await nextPaint();
      return now;
    },
    // La page anime-t-elle quelque chose ? (cumulé depuis le chargement)
    isMotion() {
      scan();
      const video = document.querySelectorAll('video').length > 0;
      return rafSeq > 0 || activity.intervals > 0 || activity.delayedTimeouts > 0 || activity.animations > 0
        || video || !!pageSeek() || !!(W.gsap && W.gsap.globalTimeline);
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
      const ok = (v) => v != null && isFinite(v) && v > 0;
      for (const k of ['duration', 'width', 'height', 'fps']) {
        let v = cfg[k] != null ? Number(cfg[k]) : meta(k);
        // Repli sur les globales du protocole seek (window.WIDTH, window.HEIGHT, window.FPS).
        if (!ok(v) && k !== 'duration' && typeof W[k.toUpperCase()] === 'number') v = W[k.toUpperCase()];
        out.meta[k] = ok(v) ? v : null;
      }
      out.seekApi = !!pageSeek();
      out.isMotion = this.isMotion();
      out.scriptDuration = (out.seekApi && ok(W.DURATION)) ? W.DURATION : null;
      return out;
    },
  };
})();
