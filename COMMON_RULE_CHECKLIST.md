# Common rules-text mistakes

Check a card's `ability` wording against this. Every item comes from a real defect
found in `../bug-vanguards`, not from general style advice.

## Joining two instructions

- [ ] **A comma cannot join two instructions.** ✗ `Remove two mouse counters from
      Bushgeh, put a land card from your hand onto the battlefield.`
      ✓ `You may remove two mouse counters from Bushgeh. If you do, put a land
      card…` Use `X, then Y` only when Y should happen even if X could not.
- [ ] **`You may` goes in front of the price, not the payoff.** ✗ `remove a war
      counter from Beagan you may put a +1/+1 counter` drains counters whether or
      not you want the effect.
- [ ] **Conditions lead.** ✗ `Draw a card, if you have no cards in hand.`
      ✓ `If you have no cards in hand, draw a card.`

## Costs

- [ ] **A cost needs an activated ability.** `Remove a mouse counter from Bushgeh:
      Draw a card.` A `:` inside a mode is always wrong — modes have effects, not
      costs: ✓ `* Sacrifice a creature. If you do, target opponent discards a card.`
- [ ] **Costs never say `target`** — you choose what to pay with as you activate.
      ✗ `Return target land you control to its owner's hand:` → ✓ `Return a land
      you control…` A tap cost says `Tap an untapped creature you control`.

## References that point at nothing

Each of these reads fine aloud, which is why they survive.

- [ ] **`target` needs a noun.** ✗ `Target you control gets +X/+X` →
      ✓ `Target creature you control`.
- [ ] **Instructions need their object.** ✗ `search your library for a basic land
      card and put into play tapped` — *put what?*
- [ ] **Back-references need an antecedent.** ✗ `whenever the chosen creature…`
      when nothing chose one; ✗ `tap that creature` on an ability targeting a
      *permanent*; ✗ `destroy the damaged creature`.
- [ ] **Distributing counters names its targets.** ✗ `Distribute three +1/+1
      counters onto creatures` → ✓ `among one, two, or three target creatures`.
- [ ] **A hand holds cards, and you can't target them.** ✗ `Put target creature
      from your hand onto the battlefield` → ✓ `a creature card from your hand`.

## Targeting, choosing, and what dies with what

- [ ] **`choose` and `target` are not interchangeable.** Targeting locks in on
      activation and respects hexproof, shroud and protection; choosing on
      resolution ignores all of it. Decide which you meant — and write `Its owner
      sacrifices it`, never `must sacrifice it`.
- [ ] **A rider after a targeted effect dies with it.** In `Counter target
      activated ability. Draw a card.` the draw is lost if the target becomes
      illegal (CR 608.2b), and it can't be activated with no legal target at all.
      For an unconditional rider use `up to one target`, which CR 115.6 lets you
      choose zero of.
- [ ] **`you` and `each opponent` follow whoever activated the ability**, not the
      vanguard's owner (CR 109.5, 602.2). Correct but invisible — don't restate
      it, don't assume otherwise. Combine restrictions as *Endbringer's Revel*
      does: `Any player may activate this ability but only as a sorcery.`

## Triggers

- [ ] **`If … would` replaces an event; `Whenever` reacts to one.** ✗ `If a spell
      you control would target a creature or player, you may draw a card`
      replaces nothing. ✓ `Whenever you cast a spell that targets…`
- [ ] **An intervening-if must exclude the event that triggered it.** ✗ `Whenever
      a land enters, if a land has already entered this turn` fires on the first
      land, not the second. ✓ `if another land entered this turn`.
- [ ] **Use the real phase name.** ✗ `At the beginning your combat step` →
      ✓ `At the beginning of combat on your turn`.

## Effects need their bounds

- [ ] **A "becomes a" effect needs a duration**, or a repeating trigger changes
      the creature *permanently*. Add `until end of turn`. Also `Bird creature`,
      not bare `Bird` — a creature type is not a type line.
- [ ] **Damage needs a source.** ✓ `Combug deals damage to that player`, not
      `that player is dealt damage` — the source decides lifelink, protection and
      redirection. Name the vanguard, as *Urza* and *Takara* do.
- [ ] **A library search always shuffles** — `…, put it onto the battlefield
      tapped, then shuffle.` The most-dropped clause on the list.
- [ ] **A face-down creature needs its full clause**: `a 2/2 creature with no
      text, no name, no subtypes, and no mana cost`.

## Names, types and capitalisation

- [ ] **A card refers to itself by name**, spelled exactly (CR 201.5) — `Bougie`
      on a card named Bouhgie refers to nothing. **When you base a card on an
      existing one, this is the line you will forget**: a card named Bugging still
      said `Urza deals 1 damage`. The short form (`Sidar Kondo` for *Sidar Kondo
      of Jamuraa*) is licensed only by a comma in the name.
- [ ] **A type a card invents is spelled the same throughout.** `Kitchin` tokens
      buffed as `Kitchen` creatures buff a type you control none of.
- [ ] **Creature types are capitalised** (`Kitchin Soldier`); **counter names and
      keywords are not** (`mouse counter`, `ninjutsu`, `storm`, `bloodthirst 2`).
- [ ] **`face down` is two words as an adverb**, hyphenated only as an adjective:
      `turn it face down`, but `face-down creatures you control`.
- [ ] **Inside granted text, say `this creature`** — `with “Sacrifice this
      creature: …”`. `Sacrifice an Insect` is not a typo but a *wider* ability.
- [ ] **Reminder text agrees with what it describes.** One token is `(It's every
      creature type.)`, not `(They're…)`.

## Grammar

- [ ] **Possessives and agreement around players.**
      ✗ `Player’s life totals` → ✓ `Players’ life totals`;
      ✗ `Creatures under opponents control` → ✓ `Creatures your opponents control`;
      ✗ `an opponent control` → ✓ `an opponent controls`;
      ✗ `each other players turn` → ✓ `each other player’s turn`.
- [ ] **No blank line inside a sentence.** `\n\n` is a paragraph break and prints
      as a 20 px gap, splitting one ability into two fragments.

## Era

- [ ] **One era per card.** `into play` is 1997; `onto the battlefield` and
      `Activate only as a sorcery` are post-2009. Mixing them in one card reads as
      a mistake — the corpus uses both, so pick per card and stay there.
      Period pair: `into play` + `Play this ability only any time you could play a sorcery.`
      Modern pair: `onto the battlefield` + `Activate only as a sorcery.`
- [ ] **`Activate only as a sorcery`** — the `as` is easy to drop.
- [ ] **`any target`, not `creature or player`.** CR 115.4 makes `any target`
      cover creature, player, planeswalker or battle — wider than the old
      phrasing, so check that widening is what you want.
- [ ] **Mana abilities don't mention the mana pool** — `{T}: Add {G}` (CR 106.4),
      unless the card is deliberately period.

## Structure and sourcing

- [ ] **A modal ability needs `choose one` and a bulleted list** (CR 700.2), each
      mode a complete instruction. The em dash is supplied by the renderer — see
      the modal-ability section in `CLAUDE.md`.
- [ ] **Never take an original's wording from Scryfall's `oracle_text`** — it is
      modernized and will not match the printed card or the reference masks. See
      [docs/scryfall.md](docs/scryfall.md).
