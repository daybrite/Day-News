// Synthetic publication fixture, executing the actual bundled touch recognizer.
// PLAYWRIGHT_MODULE=/path/to/node_modules/playwright node tests/reader-swipes.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');

(async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
    await page.setContent(`<meta name="viewport" content="width=device-width,initial-scale=1">
      <style>body{margin:0} #body{height:3000px} #wide{width:250px;overflow-x:auto} #wide div{width:800px}</style>
      <div id="body"><p id="text">Synthetic article text for selection.</p><a id="link" href="https://fixture.invalid/">Fixture link</a>
      <button id="button">Fixture control</button><div id="wide"><div>Wide publication content</div></div>
      <div contenteditable id="editable">Editable fixture</div><video id="video"></video></div>`);
    await page.evaluate(source => {
      const script = document.createElement('script');
      script.dataset.articleId = '18446744073709550001'; // IDs must not lose JS numeric precision.
      script.textContent = source;
      document.body.appendChild(script);
      window.links = [];
      document.addEventListener('click', event => {
        const anchor = event.target.closest('a');
        if (anchor) { event.preventDefault(); window.links.push(anchor.href); }
      });
      window.sequence = (options = {}) => {
        const target = document.getElementById(options.target || 'body');
        const points = options.points || [[280, 300], [240, 300], [100, 300]];
        const touch = (point, identifier = 1) => new Touch({ identifier, target, clientX: point[0], clientY: point[1] });
        const emit = (type, touches, changedTouches = touches) => target.dispatchEvent(new TouchEvent(type, {
          bubbles: true, cancelable: options.cancelable !== false, touches, changedTouches
        }));
        const first = touch(points[0]);
        emit('touchstart', options.multi ? [first, touch([200, 200], 2)] : [first]);
        for (const point of points.slice(1)) emit('touchmove', [touch(point)]);
        if (options.cancel) emit('touchcancel', [], [touch(points.at(-1))]);
        emit('touchend', [], [touch(points.at(-1))]);
        return window.links.splice(0);
      };
    }, fs.readFileSync(path.join(__dirname, '../resource/assets/reader/swipe.js'), 'utf8'));
    const sequence = options => page.evaluate(options => window.sequence(options), options);
    assert.deepEqual(await sequence(), ['day-news-reader://next/18446744073709550001']);
    assert.deepEqual(await sequence({ points: [[100, 300], [140, 300], [280, 310]] }), ['day-news-reader://previous/18446744073709550001']);
    for (const options of [
      { points: [[200, 400], [200, 370], [200, 100]] }, // vertical
      { points: [[280, 400], [280, 370], [100, 360]] }, // vertical, then horizontal
      { points: [[280, 400], [240, 370], [100, 200]] }, // diagonal
      { points: [[280, 300], [250, 300]] }, // short/abandoned
      { points: [[280, 300]] }, // tap
      { points: [[10, 300], [60, 300], [200, 300]] }, // left system edge
      { points: [[380, 300], [300, 300], [100, 300]] }, // right system edge
      { cancel: true }, { multi: true }, { cancelable: false },
      { target: 'button' }, { target: 'wide' }, { target: 'editable' }, { target: 'video' }
    ]) assert.deepEqual(await sequence(options), [], JSON.stringify(options));
    await page.evaluate(() => {
      const range = document.createRange();
      range.selectNodeContents(document.getElementById('text'));
      window.getSelection().addRange(range);
    });
    assert.deepEqual(await sequence(), [], 'selection owns the gesture');
    await page.evaluate(() => window.getSelection().removeAllRanges());
    assert.deepEqual(await sequence({ target: 'link' }), ['day-news-reader://next/18446744073709550001']);
    assert.deepEqual(await page.evaluate(() => {
      document.getElementById('link').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
      return window.links.splice(0);
    }), [], 'swiping a link does not also open it');
    await page.waitForTimeout(550);
    await page.tap('#link');
    assert.deepEqual(await page.evaluate(() => window.links.splice(0)), ['https://fixture.invalid/']);

    // Real browser input: Chromium's touch/scroll arbitration, not just DOM event dispatch.
    const cdp = await page.context().newCDPSession(page);
    async function drag(points, delay = 20) {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: points[0][0], y: points[0][1] }] });
      for (const [x, y] of points.slice(1)) {
        await page.waitForTimeout(delay);
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y }] });
      }
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    }
    const wideY = await page.locator('#wide').evaluate(node => {
      const rect = node.getBoundingClientRect();
      return rect.top + rect.height / 2;
    });
    await drag([[220, wideY], [180, wideY], [120, wideY], [60, wideY]]);
    assert.ok(await page.locator('#wide').evaluate(node => node.scrollLeft > 50), 'wide content still scrolls horizontally');
    assert.deepEqual(await page.evaluate(() => window.links.splice(0)), []);
    await drag([[280, 400], [250, 400], [200, 400], [100, 400]]);
    assert.deepEqual(await page.evaluate(() => window.links.splice(0)), ['day-news-reader://next/18446744073709550001']);
    await drag([[200, 600], [200, 550], [200, 450], [200, 250]]);
    await page.waitForTimeout(250);
    assert.ok(await page.evaluate(() => window.scrollY > 100), 'vertical scrolling remains native');
    assert.deepEqual(await page.evaluate(() => window.links.splice(0)), []);
    await drag([[280, 400], [250, 400], [100, 400]], 550);
    assert.deepEqual(await page.evaluate(() => window.links.splice(0)), [], 'long press does not navigate');
    await cdp.send('Emulation.setPageScaleFactor', { pageScaleFactor: 2 });
    assert.deepEqual(await sequence(), [], 'zoomed content can pan without navigating');
    console.log('Reader swipes passed: directions, gesture arbitration, selection, links, zoom, cancellation, exact IDs, and real browser scrolling.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
