// Synthetic publication fixtures exercising the actual bundled extraction/sanitization code.
// PLAYWRIGHT_MODULE=/path/to/node_modules/playwright node tests/reader-extraction.cjs
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || "playwright");

(async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    for (const name of ["readability.js", "purify.js", "extract.js"]) {
      await page.addScriptTag({ content: fs.readFileSync(path.join(__dirname, "../resource/assets/reader", name), "utf8") });
    }
    const paragraph = "This synthetic article explains how a telescope gathers distant light. A careful observer compares several observations, records the results, and returns the next night to check them. ";
    const html = `<html><head><title>Fixture telescope</title><base href="https://wrong.example/"></head>
      <body><nav><a href="/login">Sign in</a></nav><article><h1>Fixture telescope</h1>
      <p>${paragraph.repeat(8)}</p><p><a href="../reference">Reference</a><img src="photo.jpg" onerror="window.compromised=true"></p>
      <script>window.compromised=true</script><a href="javascript:alert(1)">Unsafe link</a>
      <iframe srcdoc="bad"></iframe><form><input name="password"></form></article></body></html>`;
    const result = await page.evaluate(input => extractArticle(input), { html, url: "https://fixture.example/posts/story" });
    assert.equal(result.title, "Fixture telescope");
    assert.ok(result.content.includes(paragraph));
    assert.ok(result.content.includes('href="https://fixture.example/reference"'));
    assert.ok(result.content.includes('src="https://fixture.example/posts/photo.jpg"'));
    assert.ok(!/onerror|javascript:|<script|<iframe|<form|<input/.test(result.content));
    assert.equal(await page.evaluate(() => window.compromised), undefined);
    assert.equal(await page.evaluate(() => extractArticle({ html: "<html><body></body></html>", url: "https://fixture.example/" })), null);
    console.log("Reader extraction: content, relative URLs, sanitization, inert scripts, and empty pages passed.");
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
