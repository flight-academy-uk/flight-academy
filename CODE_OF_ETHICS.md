# Code of Ethics

This Code of Ethics is inspired by the [SQLite Code of Ethics](https://www.sqlite.org/codeofethics.html), which is itself adapted from Chapter 4 of the *Rule of Saint Benedict* (*Regula Sancti Benedicti*, c. 540 AD) — the *Instrumenta Bonorum Operum*, "the Tools for Good Works."

Chapter 4 of the Rule lists seventy-two short precepts the monk should hold in mind. Many of them are about how to do work, how to deal with colleagues and superiors, how to be honest about one's own shortcomings, and how to attend to the long horizon. Software work and monastic work are not as different as they sound: both are slow craft, both demand humility, both are done in community over decades.

Below are the instruments from the Rule that most directly bear on the work we do here. The precept appears as Benedict wrote it (English translation; Latin where memorable). Then the brief contemporary application to this project.

This document expresses the principles by which we *hope* to conduct the Flight Academy project. They are aspirational — a direction, not a measurement. We will fall short of them; we will say so when we do.

---

**Prologue.** *Obsculta, o fili.* — "Listen, my son."

Listen to colleagues, users, regulators, and contributors before reaching for the keyboard. Aviation has a saying that the runway behind you is wasted; advice unheard is the same.

---

**§ 4.20.** *Mortificare sui ipsius voluntates.* — "To deny one's own will in order to follow Christ."

This is the first numbered instrument of Chapter 4, and it is the heaviest. The instruments that follow rest on this one; when this one fails, the rest collapse with it.

Aviation knows the cost of pilots who treat the cockpit as theirs to dominate. The Tenerife disaster of 1977 — 583 dead, still the deadliest accident in commercial aviation — was driven by a senior captain who would not be told he was wrong. Crew Resource Management, the discipline that has saved tens of thousands of lives in the decades since, exists because pride in the cockpit was killing people. The first officer who tries to correct the captain, and is shouted down, is one of the most recurring patterns in fatal accident reports.

Software is gentler in its consequences but the principle is identical. The contributor who cannot be told their code is wrong, the maintainer who cannot accept a better proposal, the architect who cannot revisit a decision when conditions change — these are the most dangerous people in any project. The codebase does not exist to be a canvas for the contributor's aesthetic.

Subordinating one's own will to something larger than oneself — for Benedict, to God; for the airline pilot, to the crew and the passengers; for us, to the project's users, to safety, and to the truth as best we can see it — is the foundation everything else rests on. When we forget this, people die in aviation. In software the cost is usually only the project, but the mechanism is the same.

> *"A superior pilot uses superior judgement to avoid situations that require the use of superior skill."*
>
> — Frank Borman, commander of Apollo 8

The same is true of every craft. The contributor who avoids the avoidable failure is more valuable than the one who heroically recovers from it. We prefer the boring fix in a quiet PR to the dramatic save at 03:00.

---

**§ 4.22.** "Not to give way to anger."

Code review at 23:00 after an outage is not the place. Sleep on it. Reply tomorrow. The bug will still be there.

> *"Rather be on the ground wishing you were in the air than in the air wishing you were on the ground."*
>
> — aviation safety maxim

The discipline of *not acting* when conditions warrant restraint is the same discipline as *not replying* when you are angry. Both are about choosing the harder, slower, grown-up option over the impulse to press on. The deploy you held last night, the PR you closed instead of merging in frustration, the angry comment you drafted and deleted — these are wins that nobody will ever see in the metrics, and they are the reason the project is still standing in five years.

---

**§ 4.24.** "Not to entertain deceit in the heart."

Do not pretend the software is more capable than it is. Do not pretend a bug is fixed when it is not. Do not write release notes that mislead.

---

**§ 4.25.** "Not to give a false peace."

If you disagree in review, say so. A reluctant approval that papers over a real concern is dishonest, even when motivated by kindness.

---

**§ 4.26.** "Not to forsake charity."

The contributor whose PR you reject is still owed your thanks for the time they spent on it. The reporter of a vulnerability is owed acknowledgement. The user filing a frustrated bug report is owed patience.

---

**§ 4.28.** "To speak the truth with heart and tongue."

Commit messages, documentation, error messages — the contributor's word given to people who will read it years from now. Write what is true. Write what you would want to find if you were the one trying to understand the code at 02:00.

---

**§ 4.35.** *Non superbum esse.* — "Not to be proud."

Inseparable from §4.20 and equally weighty. Aviation kills proud pilots; software production kills proud engineers — through bugs they would not admit, through critique they would not accept, through reviews they would not request, through warnings they would not heed. The Just Culture movement in aviation safety exists precisely because punishing humans for error makes them hide it, and hidden errors metastasise into catastrophic ones.

We work with the assumption that our own code is more likely to be wrong than the alternative we are about to dismiss. We seek review eagerly, not defensively. We thank the contributor who shows us a better way, including when the better way embarrasses us. People convinced of their own correctness build things that kill people; people who are open to being wrong build things that last.

> *"There are old pilots, and there are bold pilots, but there are no old, bold pilots."*
>
> — attributed to E. Hamilton Lee, early airmail pilot

The aphorism is older than the airline industry. It is also true in software, in security, in operations, and in management. Boldness in the face of evidence is what age teaches you to do without.

---

**§ 4.37–38.** "Not to be addicted to wine. Not to be a great eater."

Restraint. The Rule treats moderation in food and drink as a metaphor for moderation in all appetites — including features, dependencies, abstractions, and acronyms.

---

**§ 4.41.** *Otiositas inimica est animae.* — "Idleness is the enemy of the soul."

The Rule is famously firm on this. For us: tests before "fixed". Documentation before "ship it". Read before write. The fast-feeling answer is usually not the right one.

> *"The three most useless things in aviation: runway behind you, altitude above you, and fuel in the truck."*

Aviation wisdom on opportunities once lost. Software has its equivalents: the test you should have written before the bug, the architectural decision you should have written down when it was fresh, the migration you should have run at the maintenance window you let slip. *Otiositas* — the not-doing of the thing that needs doing while there is still time — is the same vice in both crafts.

---

**§ 4.42.** "Not to be a murmurer."

Do not complain about other contributors behind their backs. If a process is broken, raise it openly. If a person is causing harm, raise it through the channels in the [Code of Conduct](CODE_OF_CONDUCT.md). Gossip corrodes the work.

---

**§ 4.43.** "Not to be a detractor."

Speak well of other contributors' work where you can. If you cannot, say nothing and review the code. Praising others costs you nothing; it costs the project everything to lose them.

---

**§ 4.45–46.** "To attribute to God any good one sees in oneself. To recognise that the evil one does is one's own."

Credit prior art; cite the projects whose ideas shaped yours. Own the bugs you wrote. The Linux kernel community has a tradition of *postmortem ownership* that maps closely onto this — "I broke production" is a stronger statement than "production broke."

---

**§ 4.51.** "To keep guard at all times over the actions of one's life."

*Watchfulness* — *vigilantia*. The security-conscious instinct of always asking "what if this fails, what if this is abused, what does this look like to an attacker." In aviation: situational awareness. The two are the same instinct.

> *"Aviate, navigate, communicate."*
>
> — standard pilot training mnemonic

In any incident, fly the aircraft first. Then figure out where you are going. Then tell ATC about it. The priority order is non-negotiable, because losing track of the primary task while attending to the secondary is how aircraft are lost. The same discipline applies in any operational incident: stabilise the immediate situation, then diagnose, then communicate. Reaching for Slack before stopping the bleeding is the software-equivalent of getting on the radio while the aircraft is stalled.

---

**§ 4.52.** "To know for certain that God sees one everywhere."

A secular form of this is *behave as if your code is read by someone you respect*. Because it is — by future maintainers, by security researchers, by the regulator who eventually audits the system.

---

**§ 4.55.** "To listen willingly to holy reading."

For us: read the existing code, the regulations (CAP 382, EU 376/2014, GDPR), the related ADRs, before proposing change. Read the SQLite source, the Linux kernel discussions, the Rust standard library, when learning a new pattern.

---

**§ 4.62.** "To love chastity." *(adapted)*

Benedict means chastity of body. Applied to software: chastity of scope. Do not let unrelated changes ride along in a PR. Do not let a refactor become a rewrite. Do not let the bug fix expand into the feature you always wanted. One PR, one purpose.

> *"Plan the flight, fly the plan."*
>
> — instrument flying maxim

You decide the route, the alternates, the fuel, the limits, before you take off — because in the air, under pressure, you will make worse decisions. In software, the same is true: scope decided in a calm moment is the scope to honour when you are halfway through and tempted to widen it. The PR that lands what was proposed is more valuable than the one that lands something cleverer.

---

**§ 4.72.** "To pray for one's enemies in the love of Christ."

The bug-reporter who is rude. The reviewer who tore your PR apart. The colleague whose code you find difficult. Assume good faith; respond with grace. The project is long and most enemies are circumstantial.

---

**§ 4.73–74.** "Not to despair of God's mercy. — *Et de Dei misericordia numquam desperare.*"

The final instrument in Chapter 4. The day you write a bug that takes down production, or merge a change you should not have merged, or speak in anger to a contributor — the Rule's final word is that you may still do good work tomorrow.

---

## Aspiration

These are not rules. There is no enforcement mechanism, no committee, no review board. They describe the kind of project we are trying to build and the kind of contributors we hope to be.

When you read them and think *"I would like to be part of a project that takes these seriously"* — that is the only invitation we extend.

Benedict closes Chapter 4 with a final line worth holding onto: *Ecce sunt instrumenta artis spiritalis, quae si fuerint a nobis die noctuque incessabiliter adimpleta et in die iudicii reconsignata, illa merces nobis a Domino recompensabitur quam ipse promisit* — "Behold, these are the tools of the spiritual craft. When we have used them without ceasing day and night, and have returned them on the day of judgment, our wages will be the reward the Lord himself has promised."

We are not promised the same reward. But the *spiritual craft* — *ars spiritalis* — Benedict's name for the slow, patient, communal work of becoming better at one's vocation — that is what we aspire to here. Software is craft. Open source is community. The Rule of Saint Benedict is the oldest sustained handbook in the West for doing both at the same time.

That is why we lean on it.
