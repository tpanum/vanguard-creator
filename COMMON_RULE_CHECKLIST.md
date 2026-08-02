# Common rules-text mistakes

Check a card's `ability` wording against this. Counts in brackets are how many of
the 168 cards in `../bug-vanguards` currently trip each item.

- [ ] **No comma splice between two instructions.** A comma cannot join a price
      and what it buys — modes have effects, not costs.
      ✗ `Remove two mouse counters from Bushgeh, put a land card from your hand onto the battlefield.`
      ✓ `You may remove two mouse counters from Bushgeh. If you do, put a land card from your hand onto the battlefield.`
      Use `X, then Y` only when Y should happen even if X could not.

- [ ] **Costs belong to activated abilities.** If the design wants a real cost,
      write `Remove a mouse counter from Bushgeh: Draw a card.` — but note that
      turns an upkeep trigger into something repeatable.

- [ ] **One era per card.** `into play` and `comes into play` are 1997; `onto the
      battlefield` and `Activate only as a sorcery` are post-2009. Mixing them in
      one card reads as a mistake. The corpus is split (20 vs 22), so there is no
      house style — just be consistent within the card.
      Period pair: `into play` + `Play this ability only any time you could play a sorcery.`
      Modern pair: `onto the battlefield` + `Activate only as a sorcery.`

- [ ] **`Activate only as a sorcery`** — the `as` is easy to drop. [1]

- [ ] **`any target`, not `creature or player`.** [2] The old phrasing is dead;
      CR 115.4 defines `any target` as creature, player, planeswalker or battle,
      which is wider — check that widening is what you want.

- [ ] **Counter names are lowercase.** `mouse counter`, not `Mouse counter` —
      Oracle sets `charge counter`, `lore counter`, `rad counter` all lowercase.
      [Mouse, Ki, Gem, Fire, War counters]

- [ ] **Refer to the card by its own name**, not "this card" — `Urza deals 1
      damage`, `put a death counter on Necropotence Avatar`. Counters on a
      vanguard card are fine; *Necropotence Avatar* does exactly that.

- [ ] **A modal ability needs `choose one` and a bulleted list** (CR 700.2), each
      mode a complete instruction. The em dash is part of the template but is
      supplied by the renderer — see the modal-ability section in `CLAUDE.md`.

- [ ] **Never take an original's wording from Scryfall's `oracle_text`** — it is
      modernized and will not match the printed card or the reference masks. See
      [docs/scryfall.md](docs/scryfall.md).
