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
    const custom = await page.evaluate(input => extractArticle(input, (document, url, Readability) => {
      if (document.querySelector("script, iframe, form") || document.querySelector("[onerror]")) throw new Error("Input was not sanitized");
      if (document.baseURI !== url || typeof Readability !== "function") throw new Error("Missing hook arguments");
      return { title: "Custom fixture", byline: "Fixture author", content: '<p>Custom body <a href="../reference">Reference</a></p><img src="photo.jpg" srcset="small.jpg 1x, large.jpg 2x" onerror="window.compromised=true"><script>window.compromised=true</script>' };
    }), { html, url: "https://fixture.example/posts/story" });
    assert.equal(custom.title, "Custom fixture");
    assert.equal(custom.byline, "Fixture author");
    assert.ok(custom.content.includes("Custom body"));
    assert.ok(!/onerror|<script/.test(custom.content));
    assert.ok(custom.content.includes('href="https://fixture.example/reference"'));
    assert.ok(custom.content.includes('src="https://fixture.example/posts/photo.jpg"'));
    assert.ok(custom.content.includes('srcset="https://fixture.example/posts/small.jpg 1x, https://fixture.example/posts/large.jpg 2x"'));
    const fallback = await page.evaluate(input => extractArticle(input, document => {
      document.body.replaceChildren();
      return null;
    }), { html, url: "https://fixture.example/posts/story" });
    assert.equal(fallback.content, result.content);
    const delegated = await page.evaluate(input => extractArticle(input, (document, url, Readability) => {
      return new Readability(document, { maxElemsToParse: 50000 }).parse();
    }), { html, url: "https://fixture.example/posts/story" });
    assert.equal(delegated.content, result.content);
    for (const script of ["throw new Error('fixture');", "return {content: 5};", "return Promise.resolve({content: '<p>fixture</p>'});"]) {
      await assert.rejects(page.evaluate(({input, script}) => extractArticle(input, new Function("document", "url", "Readability", script)), { input: { html, url: "https://fixture.example/" }, script }));
    }
    assert.equal(await page.evaluate(() => window.compromised), undefined);
    console.log("Reader extraction: Mozilla, custom hooks, fallback, validation, relative URLs, sanitization, and inert publication scripts passed.");
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
