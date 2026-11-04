# Differences between the old and new systems

Every deliberate difference between the old Pi Dash (Django + old frontends)
and the new Pi Dash (Rust API + new frontend): one row per difference.
Kinds: old bug fixed, redesign, tier 3, new feature.

| ID     | Area                               | Old behaviour                                                                        | New behaviour                                                                                                                                                 | Kind        | Issue / PR  |
| ------ | ---------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| DIFF-1 | schedulers: definition color field | The color field offers only the 16 preset swatches; there is no custom-color picker. | The color field offers the same 16 presets plus a "…" (More) overflow that opens the native custom-color picker; a custom color shows on the overflow button. | new feature | NEWPIDASH-2 |
