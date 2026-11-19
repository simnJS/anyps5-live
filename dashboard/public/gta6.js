"use strict";

// GTA VI's status (stage, firmware figures, release time) comes from the dashboard: src/gta6.rs.
const RELOAD_MS = 6 * 3_600_000, CTA_MS = 15_000;
const CTAS = {
  released: ["👍 Like if you're waiting for the dump too", "🔔 Subscribe: the dump shows up here first", "💬 Already playing on PS5? Tell the chat"],
  dumped: ["👍 Like if you want GTA VI on PC", "🔔 Subscribe to see the first AnyPS5 tests", "💬 What should AnyPS5 fix first? Tell the chat"],
  playable: ["👍 Like if you're playing it on PC", "🔔 Subscribe to follow GTA VI on AnyPS5", "💬 How does it run for you? Tell the chat"],
};
const THIS_GAME = /grand theft auto\s*(vi|6)\b|\bgta\s*(vi|6)\b/i;
const number = new Intl.NumberFormat("en-US");
const fmt = (value) => (value === null || value === undefined ? "—" : number.format(value));
const escape = (text) => String(text ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const $ = (id) => document.getElementById(id);
const pad = (value) => String(value).padStart(2, "0");

// A cover is optional and supplied by whoever runs the stream: PAGE=gta6.html?cover=https://...
function showCover() {
  const url = new URLSearchParams(location.search).get("cover");
  if (!url?.startsWith("https://")) return;
  $("cover").src = url;
  $("cover").hidden = false;
  $("hero").classList.add("has-cover");
}

let gta = null, startedAt = null;

function tick() {
  const released = Date.parse(gta?.releasedAt ?? "");
  const seconds = Number.isNaN(released) ? 0 : Math.max(0, Math.floor((Date.now() - released) / 1000));
  const units = [[Math.floor(seconds / 86400), "days"], [pad(Math.floor(seconds / 3600) % 24), "hours"], [pad(Math.floor(seconds / 60) % 60), "minutes"], [pad(seconds % 60), "seconds"]];
  $("count").innerHTML = units.map(([value, label]) => `<div><strong>${value}</strong><span>${label}</span></div>`).join("");
  const ctas = CTAS[gta?.stage] ?? CTAS.released;
  $("cta").textContent = ctas[Math.floor(Date.now() / CTA_MS) % ctas.length];
  // A gameplay clip (gameplay.js) is announced a minute ahead.
  const clip = window.Gameplay?.next();
  const left = clip ? Math.max(0, Math.ceil((clip - Date.now()) / 1000)) : Infinity;
  if (left <= 60) $("cta").textContent = `🎮 GTA V gameplay in ${Math.floor(left / 60)}:${pad(left % 60)}`;
}

function renderStatus(status, thread) {
  gta = status;
  $("port-status").textContent = status.title;
  $("fw-required").textContent = status.firmware.required;
  $("fw-jailbreak").textContent = status.firmware.jailbreak;
  $("firmware").hidden = status.stage !== "released";
  $("steps").innerHTML = status.steps.map((step, index) => `<li class="${index <= status.step ? "passed" : index === status.step + 1 ? "current" : ""}">${escape(step)}</li>`).join("");
  $("port-note").textContent = thread && status.reportsCount
    ? `First AnyPS5 reports: ${thread.stage ?? "being tested"} · ${fmt(thread.replies)} replies in its test thread.`
    : status.detail;
}

const stageClass = (stage) => (!stage ? "open" : stage === "In game" ? "" : "early");
let mapKey = "";

function render(state) {
  const thread = state.games?.rockstar?.find((game) => THIS_GAME.test(game.name));
  if (state.gta6) renderStatus(state.gta6, thread);
  $("stat-done").textContent = fmt(state.progress?.done);
  $("stat-gain").textContent = state.progress?.day ? `+${fmt(state.progress.day.done)} in 24 h` : "";
  $("stat-games").textContent = fmt(state.games?.inGame);
  $("stat-merged").textContent = fmt(state.github?.merged24h);
  const games = (state.games?.rockstar ?? []).filter((game) => !THIS_GAME.test(game.name)).slice(0, 6);
  $("games").innerHTML = games.map((game) => `<li><span class="gta-game-name">${escape(game.name)}</span><span class="gta-game-stage ${stageClass(game.stage)}">${escape(game.stage ?? "Being tested")}</span></li>`).join("");
  const pr = state.github?.mergedPrs?.[0];
  $("latest").innerHTML = pr ? `<strong>Latest change:</strong> ${escape(pr.title)} · ${escape(pr.author)}` : "";
  const libraries = state.libraries?.all;
  const key = libraries?.map((library) => `${library.name}:${library.done}`).join(",");
  if (key && key !== mapKey) {
    mapKey = key;
    drawLibraryMap($("map"), libraries);
  }
}

async function refresh() {
  try {
    const response = await fetch("/api/state", { cache: "no-store" });
    if (!response.ok) return;
    const state = await response.json();
    // A restarted dashboard is a deploy: reload, so the stream shows the change within a refresh.
    if (startedAt && state.startedAt !== startedAt) reload();
    startedAt = state.startedAt;
    render(state);
  } catch (error) {
    console.error("refresh", error);
  }
}

showCover();
tick();
setInterval(tick, 1000);
refresh();
setInterval(refresh, 15_000);
// A clip playing at reload time ends first.
function reload() {
  if (window.Gameplay?.playing()) setTimeout(reload, 10_000);
  else location.reload();
}
setTimeout(reload, RELOAD_MS);
