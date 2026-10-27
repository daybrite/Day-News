# Stopwords

The word cloud on the dashboard leaves out the most common words of a feed's language: articles,
pronouns, prepositions and the like. These lists are those words, one per line, in the
languages the cloud recognizes.

- **Source:** [stopwords-iso](https://github.com/stopwords-iso/stopwords-iso),
  `stopwords-iso.json` at commit `6e8e9a2d1c019ad2a885bcbe157628dea624dfb6` (2020-09-17).
- **License:** MIT, copyright (c) 2020 Gene Diaz. The license text is in [LICENSE](LICENSE).
- **Changes:** one file per ISO 639-1 language code, each list sorted with duplicates and blank
  entries removed. Chinese, Japanese, Korean and Thai are left out: the search index does not
  split those scripts into words, so their lists would never match a term.
