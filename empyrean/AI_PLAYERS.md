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
- **Landblocks.** An AI Player keeps only its own landblock awake, not the eight round it as a
  real player does, so a walking hunter's trail sleeps behind it.
- **Settings.** `[ai_players]` in `empyrean.toml`: `leave_after_seconds` (300), `max_total` (150),
  `fade_from_players` (300: past that many real players, each one more takes one AI Player away,
  one every 5 seconds, the farthest from real players first) and `town_levels`.
- **Build.** A new AI Player is made by the `playtest-character` pipeline: a level, an archetype,
  its XP and credits spent, and gear rolled from the tier it hunts at. Spells are its trained
  schools up to what it casts reliably. `SpellComponentsRequired` off and `SafeSpellComponents` on.
- **Movement.** AI Players move as a client does, with the motion and position messages a client
  sends, so watchers see them run. Routes come from `dereth-navigation`, outdoors only.
- **Levels.** The creatures of every outdoor landblock with their levels, from its encounters and
  generators, worked out as asked; town levels from the land around them, overridable in TOML.
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
3. Done. Movement: AI Players send a client's motion and position messages through the server's
   own handlers, along routes from the client's planner (`dereth-navigation`) on a thread of its
   own; trips longer than 180 m go in hops, and a body stopped by something replans. Town AI
   Players wander within 40 m of the centre, stand a while, and now and then sit. A new area's
   first route takes seconds to plan (its map is built once); later ones take milliseconds.
4. Done. Level map: each outdoor landblock's creatures, from its encounters and outdoor
   generators (followed four deep), with their levels and chances, worked out as asked. A town's
   level is the median of the creatures in and next to its landblock (Holtburg 8, Arwic 15,
   Fort Tethana 100), unless `[ai_players] town_levels` sets it.
5. Done. Hunters: the director serves whichever is shorter, a town near a real player or the
   real player with the fewest hunters within 5 km (10 at most). A hunter's ground is a landblock
   with monsters within 5 km of the player and at least 150 m from every real player; its level is
   the ground's median monster level times 1.15, plus 3. It logs in at the nearest town, walks out,
   and takes on the nearest monster on its ground (within 70 m of the centre, up to 3 levels above
   itself, on awake land, not another hunter's, not within 40 m of a real player, not hurt by or
   fighting a real player) in melee, with its bow or with its best single-target war or void
   spell. It binds its wounds below 70% health out of a fight and 35% in one (its kits never run
   out), its arrows are topped up, and after a death it walks back from its lifestone. A real
   player no hunter could be placed near is passed over for two minutes.
6. Town buffers.
7. Tests and soak runs with AI Players on.

## Load

Two load runs, by hand, in release (`ai_players_load`, `ai_players_hunter_load` in the test kit;
the commands are in their docs). The world's tick has 16.7 ms at 60 a second. Measured on an
18-core Mac, End of Retail content.

Town AI Players only, spread over all 33 towns:

| AI Players | tick avg | tick p95 | landblocks | memory |
|---|---|---|---|---|
| 50 | 1.3 ms | 2.4 ms | 318 | 1.1 GB |
| 300 | 1.4 ms | 2.3 ms | 352 | 1.2 GB |

299 stand-in players spread as far apart as the land allows (none farther than 1.9 km from one),
towns and hunters filled round them, each cap played two minutes at real speed. Bots waking their
whole 3x3 (before) and only their own landblock (after):

| cap | hunters | tick avg | tick p95 | worst | landblocks | memory |
|---|---|---|---|---|---|---|
| 100, before | 79 | 3.5 ms | 6.6 ms | 39 ms | 955 | 1.8 GB |
| 200, before | 158 | 10.5 ms | 15.6 ms | 107 ms | 1,889 | 2.4 GB |
| 300, before | 234 | 13.8 ms | 20.4 ms | 439 ms | 2,530 | 2.9 GB |
| 200, after | 159 | 4.6 ms | 9.4 ms | 91 ms | 1,368 | 2.2 GB |
| 300, after | 239 | 10.4 ms | 16.5 ms | 127 ms | 2,290 | 2.7 GB |

The cost follows the landblocks loaded, and most of those are hunters' trails out of town. The
stand-ins load no landblocks of their own; 299 real players would. Filling a hundred at once
costs a few ticks of 30 to 70 ms (each new AI Player is built on the world thread). The route
planner runs on up to four threads and answers about eight routes a second.

## Open

- The 1999 Launch world's loot rolls make no weapons or jewellery, so it needs its own gear.
  The 2005 world needs checking. Both cap at level 126. End of Retail comes first.
- New AI Players are built on the world thread (20 to 30 ms each in release).
- `emote_manager` panics now and then on an object already gone (caught; seen in load runs, not
  an AI Players change).
