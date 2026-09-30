// OLS site behaviour. No framework, no dependencies: this page has three
// interactions and a build step would cost more than it saves.

(function () {
  "use strict";

  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // Reveal styles are scoped to .js so the page is fully readable without script.
  document.documentElement.classList.add("js");

  /* ---------- sticky nav state ---------- */

  var nav = document.getElementById("nav");
  var toggle = document.getElementById("nav-toggle");
  var links = document.getElementById("nav-links");

  function onScroll() {
    nav.dataset.stuck = String(window.scrollY > 8);
  }

  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  /* ---------- mobile menu ---------- */

  function closeMenu() {
    links.dataset.open = "false";
    toggle.setAttribute("aria-expanded", "false");
  }

  toggle.addEventListener("click", function () {
    var open = links.dataset.open === "true";
    links.dataset.open = String(!open);
    toggle.setAttribute("aria-expanded", String(!open));
  });

  links.addEventListener("click", function (event) {
    if (event.target.closest("a")) closeMenu();
  });

  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape") closeMenu();
  });

  /* ---------- active section in the nav ---------- */

  var navLinks = Array.prototype.slice.call(links.querySelectorAll("a"));
  var sections = navLinks
    .map(function (a) {
      var id = a.getAttribute("href");
      return id && id.length > 1 ? document.querySelector(id) : null;
    })
    .filter(Boolean);

  if (sections.length && "IntersectionObserver" in window) {
    var visible = new Map();

    var spy = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          visible.set(entry.target.id, entry.isIntersecting);
        });

        // Pick the topmost section currently on screen, so the nav does not
        // flip between two entries while a section boundary is in view.
        var activeId = null;
        for (var i = 0; i < sections.length; i++) {
          if (visible.get(sections[i].id)) {
            activeId = sections[i].id;
            break;
          }
        }
        if (!activeId && window.scrollY > 240) {
          activeId = sections[sections.length - 1].id;
        }

        navLinks.forEach(function (a) {
          if (activeId && a.getAttribute("href") === "#" + activeId) {
            a.setAttribute("aria-current", "true");
          } else {
            a.removeAttribute("aria-current");
          }
        });
      },
      { rootMargin: "-30% 0px -55% 0px", threshold: 0 }
    );

    sections.forEach(function (section) {
      spy.observe(section);
    });
  }

  /* ---------- reveal on scroll ---------- */

  var reveals = document.querySelectorAll(".reveal");

  if (reduced || !("IntersectionObserver" in window)) {
    reveals.forEach(function (el) {
      el.dataset.shown = "true";
    });
  } else {
    var fade = new IntersectionObserver(
      function (entries, observer) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          // Stagger inside a row so cards arrive in sequence, not together.
          var delay = Number(entry.target.dataset.revealDelay || 0);
          setTimeout(function () {
            entry.target.dataset.shown = "true";
          }, delay);
          observer.unobserve(entry.target);
        });
      },
      { rootMargin: "0px 0px -8% 0px", threshold: 0.06 }
    );

    Array.prototype.forEach.call(reveals, function (el, index) {
      if (!el.dataset.revealDelay) {
        el.dataset.revealDelay = String((index % 3) * 70);
      }
      fade.observe(el);
    });
  }

  /* ---------- tour tabs ---------- */

  var tabs = Array.prototype.slice.call(document.querySelectorAll(".tab"));

  function selectTab(tab) {
    tabs.forEach(function (other) {
      var on = other === tab;
      other.setAttribute("aria-selected", String(on));
      other.tabIndex = on ? 0 : -1;
      var panel = document.getElementById(other.getAttribute("aria-controls"));
      if (panel) panel.dataset.active = String(on);
    });
  }

  tabs.forEach(function (tab, index) {
    tab.tabIndex = tab.getAttribute("aria-selected") === "true" ? 0 : -1;

    tab.addEventListener("click", function () {
      selectTab(tab);
    });

    tab.addEventListener("keydown", function (event) {
      var next = null;
      if (event.key === "ArrowDown" || event.key === "ArrowRight") {
        next = tabs[(index + 1) % tabs.length];
      } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
        next = tabs[(index - 1 + tabs.length) % tabs.length];
      } else if (event.key === "Home") {
        next = tabs[0];
      } else if (event.key === "End") {
        next = tabs[tabs.length - 1];
      }
      if (!next) return;
      event.preventDefault();
      selectTab(next);
      next.focus();
    });
  });

  /* ---------- copy to clipboard ---------- */

  document.querySelectorAll(".code__copy").forEach(function (button) {
    button.addEventListener("click", function () {
      var pre = button.closest(".code").querySelector("pre");
      if (!pre) return;

      var text = pre.innerText.trimEnd();
      var done = function () {
        var original = button.textContent;
        button.textContent = "Copied";
        button.dataset.done = "true";
        setTimeout(function () {
          button.textContent = original;
          button.dataset.done = "false";
        }, 1400);
      };

      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, function () {});
        return;
      }

      // Older WebView and plain-HTTP contexts: fall back to a selection copy.
      var scratch = document.createElement("textarea");
      scratch.value = text;
      scratch.setAttribute("readonly", "");
      scratch.style.position = "absolute";
      scratch.style.left = "-9999px";
      document.body.appendChild(scratch);
      scratch.select();
      try {
        document.execCommand("copy");
        done();
      } catch {
        /* leave the button untouched rather than claim a copy that did not happen */
      }
      document.body.removeChild(scratch);
    });
  });

  /* ---------- smooth anchor scrolling, offset for the sticky nav ---------- */

  document.addEventListener("click", function (event) {
    var link = event.target.closest('a[href^="#"]');
    if (!link) return;
    var id = link.getAttribute("href");
    if (id.length < 2) return;
    var target = document.querySelector(id);
    if (!target) return;

    event.preventDefault();
    target.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" });
    if (history.replaceState) history.replaceState(null, "", id);
  });
})();
