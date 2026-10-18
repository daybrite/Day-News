// Mobile article paging. Touch events let the reader arbitrate against publication
// content before claiming a gesture; a native pan on the WebView steals its scrolling.
(() => {
  'use strict';
  const article = document.currentScript.dataset.articleId;
  let gesture = null;
  let suppressClickUntil = 0;
  const selected = () => {
    const selection = window.getSelection();
    return selection && !selection.isCollapsed;
  };
  const zoomed = () => window.visualViewport && window.visualViewport.scale > 1.01;
  const ownsTouch = target => {
    for (let node = target; node instanceof Element; node = node.parentElement) {
      if (node.matches('button, input, textarea, select, video, audio, iframe, [contenteditable]:not([contenteditable="false"]), [role="slider"]')) return true;
      // Keep native horizontal scrolling, including when already at either end.
      if (node.scrollWidth > node.clientWidth + 1 &&
          /auto|scroll|overlay/.test(getComputedStyle(node).overflowX)) return true;
    }
    return false;
  };
  document.addEventListener('touchstart', event => {
    gesture = null;
    if (event.touches.length !== 1 || selected() || zoomed() || ownsTouch(event.target)) return;
    const touch = event.touches[0];
    // Leave both screen edges to system back gestures (NNW also reserves the left edge).
    if (touch.clientX <= 40 || touch.clientX >= window.innerWidth - 40) return;
    gesture = { id: touch.identifier, x: touch.clientX, y: touch.clientY,
      started: performance.now(), horizontal: false };
  }, { passive: true });

  document.addEventListener('touchmove', event => {
    if (!gesture) return;
    if (event.touches.length !== 1 || selected() || zoomed()) { gesture = null; return; }
    const touch = event.touches[0];
    if (touch.identifier !== gesture.id) { gesture = null; return; }
    const dx = Math.abs(touch.clientX - gesture.x);
    const dy = Math.abs(touch.clientY - gesture.y);
    if (!gesture.horizontal) {
      if (dx < 12 && dy < 12) return;
      // Once vertical scrolling starts, never convert it into article navigation.
      if (dx <= dy * 1.5) { gesture = null; return; }
      gesture.horizontal = true;
    }
    // A WebView/system recognizer has already claimed this sequence.
    if (!event.cancelable) { gesture = null; return; }
    event.preventDefault();
  }, { passive: false });

  document.addEventListener('touchend', event => {
    const completed = gesture;
    gesture = null;
    if (!completed || !completed.horizontal) return;
    const touch = Array.from(event.changedTouches).find(t => t.identifier === completed.id);
    if (!touch || event.touches.length || selected() || zoomed()) return;
    // Suppress the compatibility click even if the user abandons a horizontal swipe.
    if (event.cancelable) event.preventDefault();
    suppressClickUntil = performance.now() + 500;
    const dx = touch.clientX - completed.x;
    const dy = touch.clientY - completed.y;
    if (Math.abs(dx) < 64 || Math.abs(dx) <= Math.abs(dy) * 1.5 ||
        performance.now() - completed.started > 1000) return;
    const link = document.createElement('a');
    link.href = `day-news-reader://${dx < 0 ? 'next' : 'previous'}/${article}`;
    link.hidden = true;
    document.body.appendChild(link);
    link.click();
    link.remove();
  }, { passive: false });

  document.addEventListener('touchcancel', () => { gesture = null; }, { passive: true });
  document.addEventListener('click', event => {
    // Programmatic navigation above has detail=0; ordinary link taps remain native.
    if (event.detail && performance.now() < suppressClickUntil) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  }, true);
})();
