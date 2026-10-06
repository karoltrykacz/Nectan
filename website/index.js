const btn = document.querySelector('#theme-toggle');
const html = document.documentElement;

const saved = localStorage.getItem('theme');
const osDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
html.dataset.theme = saved || (osDark ? 'dark' : 'light');

btn.addEventListener('click', () => {
  const next = html.dataset.theme === 'dark' ? 'light' : 'dark';
  html.dataset.theme = next;
  localStorage.setItem('theme', next);
});

const LOCALES = [
  { code: "en", name: "English" },
  { code: "pl", name: "Polish" },
];

const FALLBACK = "en";
const cache = {};

function getNested(obj, path) {
  return path.split(".").reduce((o, k) => (o && o[k] !== undefined ? o[k] : undefined), obj);
}

async function loadLocale(code) {
  if (cache[code]) return cache[code];
  const res = await fetch(`/locales/${code}.json`);
  if (!res.ok) throw new Error(`Missing locale file: ${code}`);
  const data = await res.json();
  cache[code] = data;
  return data;
}

function applyTranslations(dict) {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    const val = getNested(dict, key);
    if (val !== undefined) el.textContent = val;
  });

  document.querySelectorAll("[data-i18n-attr]").forEach((el) => {
    let map;
    try {
      map = JSON.parse(el.getAttribute("data-i18n-attr"));
    } catch {
      return;
    }
    Object.entries(map).forEach(([attr, key]) => {
      const val = getNested(dict, key);
      if (val !== undefined) el.setAttribute(attr, val);
    });
  });
}

async function setLocale(code) {
  let dict;
  try {
    dict = await loadLocale(code);
  } catch {
    if (code !== FALLBACK) return setLocale(FALLBACK);
    return;
  }
  applyTranslations(dict);
  document.documentElement.setAttribute("lang", code);
  localStorage.setItem("lang", code);
  document.dispatchEvent(new CustomEvent("localechange", { detail: code }));
  const label = document.getElementById("lang-current-label");
  if (label) label.textContent = LOCALES.find((l) => l.code === code)?.name ?? code;
}

function detectInitialLocale() {
  const saved = localStorage.getItem("lang");
  if (saved && LOCALES.some((l) => l.code === saved)) return saved;
  const nav = (navigator.language || FALLBACK).slice(0, 2);
  return LOCALES.some((l) => l.code === nav) ? nav : FALLBACK;
}

function buildSelector() {
  const wrap = document.createElement("div");
  wrap.className = "lang-select";
  wrap.innerHTML = `
    <button type="button" class="lang-select-btn" aria-haspopup="listbox" aria-expanded="false">
      <span id="lang-current-label">English</span>
    </button>
    <div class="lang-select-panel" hidden>
      <input type="text" class="lang-select-search" placeholder="Search language…" />
      <ul class="lang-select-list" role="listbox"></ul>
    </div>
  `;

  const btn = wrap.querySelector(".lang-select-btn");
  const panel = wrap.querySelector(".lang-select-panel");
  const search = wrap.querySelector(".lang-select-search");
  const list = wrap.querySelector(".lang-select-list");

  function renderList(filter = "") {
    const q = filter.trim().toLowerCase();
    list.innerHTML = "";
    LOCALES.filter((l) => l.name.toLowerCase().includes(q) || l.code.includes(q))
      .forEach((l) => {
        const li = document.createElement("li");
        li.textContent = l.name;
        li.setAttribute("role", "option");
        li.tabIndex = 0;
        li.addEventListener("click", () => {
          setLocale(l.code);
          closePanel();
        });
        list.appendChild(li);
      });
  }

  function openPanel() {
    panel.hidden = false;
    btn.setAttribute("aria-expanded", "true");
    renderList();
    search.value = "";
    search.focus();
  }
  function closePanel() {
    panel.hidden = true;
    btn.setAttribute("aria-expanded", "false");
  }

  btn.addEventListener("click", () => (panel.hidden ? openPanel() : closePanel()));
  search.addEventListener("input", (e) => renderList(e.target.value));
  document.addEventListener("click", (e) => {
    if (!wrap.contains(e.target)) closePanel();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closePanel();
  });

  return wrap;
}

document.addEventListener("DOMContentLoaded", () => {
  const mount = document.querySelector(".tabs") || document.body;
  mount.appendChild(buildSelector());
  setLocale(detectInitialLocale());
});
