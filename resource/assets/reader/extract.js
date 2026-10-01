// Day-News local extraction adapter. Publication HTML is parsed in a detached document.
// Readability is not a sanitizer: sanitize before parsing and again before rendering.
function extractArticle(input) {
  const clean = DOMPurify.sanitize(input.html, {
    WHOLE_DOCUMENT: true,
    USE_PROFILES: { html: true },
    ADD_TAGS: ["title"],
    FORBID_TAGS: ["base", "style", "link", "iframe", "form", "input", "button"],
    FORBID_ATTR: ["style", "srcdoc"],
  });
  const doc = new DOMParser().parseFromString(clean, "text/html");
  const base = doc.createElement("base");
  base.href = input.url;
  doc.head.prepend(base);
  const article = new Readability(doc, { maxElemsToParse: 50000 }).parse();
  if (!article || !article.textContent.trim()) return null;
  return {
    content: DOMPurify.sanitize(article.content, {
      USE_PROFILES: { html: true },
      FORBID_TAGS: ["style", "link", "iframe", "form", "input", "button"],
      FORBID_ATTR: ["style", "srcdoc"],
    }),
    title: article.title || null,
    byline: article.byline || null,
    url: input.url,
  };
}
