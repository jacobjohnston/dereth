# AI Players

Server-controlled characters that look and act like players: they fill towns near real players,
hunt the land around them, and buff real players who lack buffs. "AI" means rule-driven bots, not
language models. This is a local feature (`local/ai-players`), not for upstream.

## Rules

- AI Players log in only within 5 km of a real player, and log out when none is within 6 km for a
  while. A real player in a dungeon counts from where they last stood outdoors.
- Towns within range fill to their size tier (hub, town, outpost; TOML override) with AI Players
  of the town's level up to 20 levels above it.
- The wilderness holds at most 10 AI Players per 5 km, each soloing monsters of its own level.
  Hunters log in at the nearest town and walk out.
- No AI Player enters a dungeon, speaks or replies.
- Town AI Players who can cast buffs give real players nearby the buffs they lack.
- AI Players never run out of spell components, arrows, kits or mana.
- AI Players leave alone any monster a real player is fighting or has hurt in the last 3 minutes,
  and keep their distance from real players.
- `@aiplayers on|off|status|list`, in game (Admin) and on the server console. Off logs every AI
  Player out; on logs them in one at a time, at most one every 5 seconds. The switch is the server
  property `ai_players_enabled`, so it survives a restart.
- An AI Player's name starts with `~`, as an admin's starts with `+`, so players know it is a bot.
  A tell to one is answered by the server, not the bot.
- AI Players count nowhere players are counted: the server list, the status ping and `/status`,
  the Dereth Network, `pop`, channel lists, friends, allegiances, the spell economy and the
  shutdown wait.

## Design

- **Identity.** AI Players are real characters on one account, `aiplayers`, which no client can
  log into. The pool is saved, so the same names come back. Names come from `names.txt`, about a
  thousand built from the 1999 creation screens' naming help for the three first heritages.
- **Session.** Each AI Player is a Player with a stub session: a `SessionData` whose id the
  transport does not know, so what is sent to it is dropped. The world's own tests log players in
  this way.
- **Director.** On the world thread every few seconds: gather real players outdoors, work out the
  towns and wilderness each one needs filled, and queue logins and log-offs.
- **Brain.** Each AI Player reads the world directly and acts through the same handlers a
  client's messages reach (combat mode, attack, cast, use).
- **Build.** A new AI Player is made by the `playtest-character` pipeline: a level, an archetype,
  its XP and credits spent, and gear rolled from the tier it hunts at. Spells are its trained
  schools up to what it casts reliably. `SpellComponentsRequired` off and `SafeSpellComponents` on.
- **Movement.** AI Players move as a client does, with the motion and position messages a client
  sends, so watchers see them run. Routes come from `dereth-navigation`, outdoors only.
- **Levels.** A table of expected monster level for every outdoor landblock, built at startup from
  encounters and generators; town levels from the land around them, overridable in TOML.
- **Kills and deaths.** AI Player kills drop no loot. AI Players drop nothing, take no vitae and
  gain no XP.

## Phases

0. Done. Branch, plan, `~` marker, `@aiplayers`, the switch, AI Players in at a town and out.
1. Done. Identity: account, names, random looks, archetype builds, gear, spells up to the level
   each casts nine times in ten, unlimited components and mana. Arrows and kits come with the
   hunters, who use them.
2. Done. Uncounted and unchanged: no vitae, no items dropped, no experience, no loot or rares from
   their kills, nothing spent of the spell economy or spell charge; trades, gifts, friends,
   fellowships and oaths turned away with a message; left out of channel lists.
3. Movement: client-style walking and routes.
4. Level map and town levels.
5. Hunters.
6. Town buffers.
7. Tests and soak runs with AI Players on.

## Open

- The 1999 Launch world's loot rolls make no weapons or jewellery, so it needs its own gear.
  The 2005 world needs checking. Both cap at level 126. End of Retail comes first.
- Each AI Player keeps its 3x3 landblocks loaded: a global cap, measured with the soak runs.
