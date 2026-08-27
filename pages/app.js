(() => {
  const root = document.documentElement;
  const themeMeta = document.querySelector('meta[name="theme-color"]');
  const toasts = document.getElementById("toasts");
  const key = "shovel-pages-theme";

  function theme() {
    return root.classList.contains("theme-light") ? "light" : "dark";
  }

  function applyTheme(next) {
    const value = next === "light" ? "light" : "dark";
    root.classList.toggle("theme-light", value === "light");
    root.classList.toggle("theme-dark", value === "dark");
    if (themeMeta) {
      themeMeta.setAttribute("content", value === "light" ? "#e4e8ee" : "#14161b");
    }
    try {
      localStorage.setItem(key, value);
    } catch {
      /* ignore */
    }
  }

  function toast(message) {
    if (!toasts) return;
    const el = document.createElement("div");
    el.className = "toast";
    el.textContent = message;
    toasts.append(el);
    window.setTimeout(() => el.remove(), 2000);
  }

  function copyText(text) {
    const done = () => toast("Copied");
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(() => fallback(text, done));
    } else {
      fallback(text, done);
    }
  }

  function fallback(text, done) {
    const area = document.createElement("textarea");
    area.value = text;
    document.body.append(area);
    area.select();
    try {
      document.execCommand("copy");
      done();
    } catch {
      toast("Copy failed");
    }
    area.remove();
  }

  document.addEventListener("click", (event) => {
    if (event.target.closest("[data-theme-toggle]")) {
      applyTheme(theme() === "dark" ? "light" : "dark");
      return;
    }
    const copy = event.target.closest("[data-copy]");
    if (copy) copyText(copy.getAttribute("data-copy") || "");
  });

  try {
    const saved = localStorage.getItem(key);
    if (saved === "light" || saved === "dark") applyTheme(saved);
  } catch {
    /* ignore */
  }
})();
