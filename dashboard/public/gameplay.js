"use strict";

// GTA V gameplay recorded on AnyKyty, played full screen on the clock: every `everyMinutes` (at :00 and :30 for 30),
// for the clip's length or `maxMinutes`. The clip and its texts are in the gameplay folder, next to the code on the
// server: gameplay.json, written by tools/gameplay.sh with the video. No clip there, no gameplay.
// The clip is muted: the stream keeps its own soundtrack, and the game's radio would draw copyright claims.
// ?gameplay=now plays it at once, to try it.
(() => {
  const CONFIG_URL = "/gameplay/gameplay.json";
  const CONFIG_MS = 60_000, PRELOAD_MS = 45_000, LATE_START_MS = 5_000, FADE_MS = 600;
  const playNow = new URLSearchParams(location.search).get("gameplay") === "now";
  let config = null, playing = false, lastSlot = 0, stopTimer = 0, limitMs = 0;

  const overlay = document.createElement("div");
  overlay.className = "gameplay";
  overlay.innerHTML = `<video muted playsinline preload="auto"></video>
    <div class="gameplay-badge"><span class="gameplay-tag">▶ Gameplay</span><div><div class="gameplay-title"></div><div class="gameplay-caption"></div></div></div>
    <div class="gameplay-time"></div>
    <div class="gameplay-bar"><span></span></div>`;
  document.body.append(overlay);
  const video = overlay.querySelector("video");
  const $ = (selector) => overlay.querySelector(selector);

  const every = () => Math.max(5, Number(config?.everyMinutes) || 30) * 60_000;
  const clipUrl = () => `/gameplay/${encodeURIComponent(config.file)}`;
  const emit = (name) => document.dispatchEvent(new CustomEvent(`gameplay:${name}`, { detail: config }));
  const clock = (seconds) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

  async function loadConfig() {
    try {
      const response = await fetch(CONFIG_URL, { cache: "no-store" });
      const next = response.ok ? await response.json() : null;
      if (next?.file !== config?.file) release();
      config = next?.file ? next : null;
    } catch (error) {
      config = null;
    }
    if (playNow && config && !lastSlot) start(Date.now());
  }

  // The clip is loaded shortly before its slot and let go after it, so the browser holds no decoder in between.
  function preload() {
    if (video.getAttribute("src") !== clipUrl()) {
      video.src = clipUrl();
      video.load();
    }
  }

  function release() {
    if (playing || !video.getAttribute("src")) return;
    video.removeAttribute("src");
    video.load();
  }

  async function start(slot) {
    lastSlot = slot;
    playing = true;
    preload();
    video.currentTime = 0;
    $(".gameplay-title").textContent = config.title ?? "GTA V gameplay";
    $(".gameplay-caption").textContent = config.caption ?? "";
    try {
      await video.play();
    } catch (error) {
      console.error("gameplay", error);
      finish();
      return;
    }
    const clipMs = Number.isFinite(video.duration) ? video.duration * 1000 : 10 * 60_000;
    limitMs = Math.min(clipMs, (Number(config.maxMinutes) || Infinity) * 60_000);
    stopTimer = setTimeout(finish, limitMs + 500);
    overlay.classList.add("show");
    document.body.classList.add("gameplay-on");
    emit("start");
  }

  function finish() {
    if (!playing) return;
    clearTimeout(stopTimer);
    overlay.classList.remove("show");
    document.body.classList.remove("gameplay-on");
    setTimeout(() => {
      video.pause();
      playing = false;
      release();
      emit("end");
    }, FADE_MS);
  }

  video.addEventListener("ended", finish);
  video.addEventListener("error", () => playing && finish());
  video.addEventListener("timeupdate", () => {
    if (!limitMs) return;
    const done = Math.min(1, (video.currentTime * 1000) / limitMs);
    $(".gameplay-bar > span").style.width = `${(done * 100).toFixed(2)}%`;
    $(".gameplay-time").textContent = clock(Math.max(0, limitMs / 1000 - video.currentTime));
  });

  function tick() {
    if (!config || playing) return;
    const now = Date.now(), slot = Math.floor(now / every()) * every();
    if (now - slot < LATE_START_MS && slot !== lastSlot) start(slot);
    else if (slot + every() - now < PRELOAD_MS) preload();
  }

  // For the pages: when the next clip starts (null without one), and whether one plays now.
  window.Gameplay = {
    next: () => (config ? Math.floor(Date.now() / every()) * every() + every() : null),
    playing: () => playing,
    title: () => config?.title ?? "GTA V gameplay",
  };

  loadConfig();
  setInterval(loadConfig, CONFIG_MS);
  setInterval(tick, 1000);
})();
