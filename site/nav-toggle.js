(() => {
  "use strict";

  const navToggle = document.querySelector(".nav-toggle");
  const primaryNav = document.querySelector("#primary-nav");

  function setNavOpen(open) {
    navToggle?.setAttribute("aria-expanded", String(open));
    primaryNav?.classList.toggle("is-open", open);
  }

  navToggle?.addEventListener("click", () => {
    setNavOpen(navToggle.getAttribute("aria-expanded") !== "true");
  });

  primaryNav?.addEventListener("click", (event) => {
    if (event.target.closest("a")) setNavOpen(false);
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") setNavOpen(false);
  });
})();
