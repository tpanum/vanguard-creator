# Scryfall

Reference for looking up Vanguard card data by hand. `vgc` never calls Scryfall at
runtime — there is no HTTP client in `Cargo.toml`, and every asset it needs is
bundled. This is an authoring aid.

Base URL `https://api.scryfall.com`. No key, no auth. Send a `User-Agent`, keep to
~10 requests/second, and cache what you fetch. Errors come back as JSON with
`"object": "error"` and a `details` string, so a 404 still tells you what it looked for.

## Finding the card

The 1997 set is `pvan` (32 cards). `t:vanguard` alone returns 107, including the MTGO
avatars, so scope the query:

```sh
curl -sA vgc "https://api.scryfall.com/cards/search?q=t%3Avanguard+set%3Apvan+name%3Aorim"
```

When you know the exact name, `/cards/named` skips the result list:

```sh
curl -sA vgc "https://api.scryfall.com/cards/named?exact=gerrard&set=pvan"
```

Search responses paginate at 175 cards; the whole set fits on one page
(`has_more: false`). Add `&format=csv` for a flat table of the set.

## Reading the text

```sh
curl -sA vgc "https://api.scryfall.com/cards/named?exact=gerrard&set=pvan" \
  | python3 -c 'import json,sys; c=json.load(sys.stdin); print(c["hand_modifier"], c["life_modifier"]); print(c["flavor_text"])'
```

Trustworthy for a card YAML:

| Field | Goes to |
| --- | --- |
| `hand_modifier` | left bubble, e.g. `"-4"` |
| `life_modifier` | right bubble, e.g. `"+0"` |
| `flavor_text` | `flavor:` — italics come through as `*asterisks*` |

**`oracle_text` is not the rules text.** Scryfall stores only modernized wording
("from the battlefield" where the card says "from play"). It differs from what is
printed, and therefore from the reference masks the accuracy suite scores against.
Take rules text from the scan.

Scryfall has no `color:` for the gem either — that is a property of the printed
sphere, measured from the scan. See the gem section in [CLAUDE.md](../CLAUDE.md).

## Looking at the card

Fastest path to the artwork — `format=image` redirects straight to the file, so
`-L` and an image viewer are the whole workflow:

```sh
curl -sLA vgc "https://api.scryfall.com/cards/named?exact=gerrard&set=pvan&format=image&version=png" -o gerrard.png
open gerrard.png
```

`version=` picks the crop: `png` (highest resolution, rounded corners), `large`,
`normal`, `small`, `art_crop` (illustration only — the useful one when sourcing art
for a card), `border_crop`. The same URLs sit in the card object's `image_uris` if
you want them without a second request.

To read the card in a browser instead, every card object carries `scryfall_uri`:

```sh
open "$(curl -sA vgc 'https://api.scryfall.com/cards/named?exact=gerrard&set=pvan' | python3 -c 'import json,sys; print(json.load(sys.stdin)["scryfall_uri"])')"
```

Scryfall's own images are scans of the same printings in `tests/assets/`, at lower
resolution and with a different colour cast. Use them to check wording and layout,
never as a reference mask.
