// Upgrades the paper texture to the full-resolution scan once it has loaded,
// so first paint gets the small fast tile (~22KB) and the page settles on the
// detailed one (~445KB) a moment later.
//
// The flag matters: without it this swap re-runs on every single navigation,
// starting each page on the low-res tile and visibly snapping to the sharp one
// even though it was already cached. Once the full-res version is known to be
// cached, the inline script in <head> applies it before first paint instead,
// so there's nothing to see.
(() => {
  const FULL = "/assets/art/paper_bg_100.webp";
  const img = new Image();
  img.onload = () => {
    document.documentElement.style.setProperty("--paper-bg", `url("${FULL}")`);
    try {
      localStorage.setItem("rh_paper_hires", "1");
    } catch (e) {
      /* private mode — just means we upgrade again next page */
    }
  };
  img.src = FULL;
})();
