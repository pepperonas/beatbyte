# Asking a catalogue

BeatByte can ask MusicBrainz which recording a song is. It is off by
default, it is the only part of the offline tool that talks to a
network, and **a song is fully playable without ever using it**.

```bash
cargo build -p beatbyte-cli --features catalogue
beatbyte-cli library songs/imported --catalogue --dry-run   # look first
beatbyte-cli library songs/imported --catalogue
```

What leaves the machine is an artist and a title. What comes back is
recorded as a claim: the recording's identifier, plus a line in the
document's analysis log saying which catalogue said so and when.

## Why the score cannot be believed

Asked for David Bowie's "Heroes", MusicBrainz returns eight
recordings on the first page. **Every one of them is scored 100.**
Their lengths run from 0 to 393 seconds: live takes from four
different tours, a 2021 re-release, the album version, and the single
edit. The score cannot tell them apart, because as text they are
identical.

The song's own length can. That is what decides here, using the same
rule the lyrics lookup uses — the larger of an absolute and a
relative allowance — so a recording accepted here is one whose length
really is this song's.

⚠️ **Without our own length, nothing is chosen at all.** There is no
evidence left, and a guess written into a document looks exactly like
knowledge.

## Why unlabelled recordings come first

Asked for Bruce Springsteen's "Born to Run", the first twelve answers
are twelve live takes; the 4:31 everybody means is not among them. Of
2338 recordings, ten carry no disambiguation, and one of those is the
studio take.

So the choice is made in **tiers**: recordings the catalogue felt no
need to label are considered first, and a labelled one — live, demo,
remix, karaoke — is reached only when none of the others fits. A soft
penalty was tried first and was not enough: a live take whose length
happens to sit a second nearer still won.

A song really can BE the live take, so the fallback exists — but a
match reached that way has its confidence cut to
`FALLBACK_CONFIDENCE` (0.4), which is below the bar for writing
anything down. The confidence says which tier it came from, not only
how close the length was.

## What is written, and what is not

**Written:** the recording's MusicBrainz id, above a confidence of
**0.9**, together with a `catalogue` run in the analysis log.

**Not written: the album and the release year.** ⚠️ Measured over
this library, the release list in a search answer is an arbitrary
handful of the releases a recording appears on, and that handful is
usually a sampler:

| Song | What the answer offered |
|---|---|
| All That She Wants | *Dance DeLuxe* (1993) |
| Don't Stop Believin' | *Kulthits: Internationale Ohrwürmer* (2021) |
| Life Is a Flower | *Beautiful Life – The Singles* (2023) |
| Born to Run | *Born to Run* (1975) ✓ |

Roughly three of eight were the real album. An identifier is a fact
about which recording this is; an album taken from that list is a
guess that would read as a fact. Getting it right needs the
recording's full release list, which is a second request per song —
filed as **M9b**.

## Being a good guest

- **A descriptive `User-Agent`**, naming the application, its version
  and where to find its author. The service blocks anonymous agents,
  and is right to.
- **At most one request every 1.1 seconds**, enforced inside the
  client so a caller cannot forget. The rate limit is the service's
  rule, not the caller's preference.
- **A 503 is a "wait", not an answer.** Four of eighty-six requests
  were refused that way even inside the interval; they are waited out
  (3 s, then 6 s) and asked again. A 404 is an answer and is not
  repeated.
- **Nothing is asked twice.** A song whose document already carries
  an id is skipped, and so is a study twin — it is the same recording
  as the song it practises.

## What it found here

86 songs asked (twins excluded), 67 matched, 16 too thin to write,
**52 given an identifier**. The rest keep their gaps, which is the
correct outcome for a song the catalogue could not place.
