# Project Context: TextMate Tokenizer

Never modify anything in grammars-themes folder
If you see an `assert!(false)` in a test, don't remove it.
Always look at ../pulls/vscode-textmate if you have doubts on the algorithm or if I mention vscode-textmate
Do not do temporary fixes/hacks if the issue is somewhere else.
Do not create new files for tests, add them to existing ones.
There is a debug feature that you can enable to get tracing debug for the tokenization

## Grammar Statistics (Latest Analysis)

```
Starting grammar and scope analysis...
Extracting scopes from grammars-themes/packages/tm-grammars/grammars...
Found 15394 raw grammar scopes
Found 0 raw theme scopes
Generated 24225 total scopes (including hierarchy)
Scope extraction complete!
- Found 24225 unique scopes

Performing atom analysis...

=== ATOM ANALYSIS ===
Total unique atoms: 3500
Total scopes analyzed: 24225
Average atoms per scope: 3.94

--- Distribution by Atom Count ---
Scopes with 1 atom: 356 (1.5%)
Scopes with 2 atoms: 1518 (6.3%)
Scopes with 3 atoms: 6556 (27.1%)
Scopes with 4 atoms: 9451 (39.0%)
Scopes with 5 atoms: 4498 (18.6%)
Scopes with 6 atoms: 1234 (5.1%)
Scopes with 7 atoms: 421 (1.7%)
Scopes with 8 atoms: 114 (0.5%)
Scopes with 9 atoms: 57 (0.2%)
Scopes with 10 atoms: 12 (0.0%)
Scopes with 11 atoms: 6 (0.0%)
Scopes with 12 atoms: 2 (0.0%)

--- Most Common Atoms (Top 20) ---
1. keyword (5093)
2. meta (4265)
3. punctuation (4079)
4. definition (2249)
5. other (2233)
6. operator (2154)
7. constant (2133)
8. function (1807)
9. type (1665)
10. entity (1642)
11. support (1631)
12. string (1478)
13. control (1434)
14. name (1361)
15. end (1350)
16. variable (1271)
17. begin (1174)
18. storage (1090)
19. section (975)
20. comment (918)

Performing capture analysis...

=== CAPTURE SCOPE ANALYSIS ===
Total scopes with captures: 410 (1.7% of all scopes)
Regular scopes: 23815 (98.3%)

--- Transformation Usage ---
/downcase: 15 scopes
/upcase: 0 scopes
/capitalize: 0 scopes

Performing longest scopes analysis...

=== LONGEST SCOPES ANALYSIS ===
Top 20 scopes with most atoms:

1. punctuation.section.block.begin.bracket.curly.function.definition.special.member.destructor.cpp (12 atoms) - from cpp.json
2. punctuation.section.block.end.bracket.curly.function.definition.special.member.destructor.cpp (12 atoms) - from cpp.json
3. punctuation.section.block.begin.bracket.curly.function.definition.special.constructor.cpp (11 atoms) - from cpp.json
4. punctuation.section.block.begin.bracket.curly.function.definition.special.member.destructor (11 atoms) - from cpp.json
5. punctuation.section.block.begin.bracket.curly.function.definition.special.operator-overload.cpp (11 atoms) - from cpp.json
6. punctuation.section.block.end.bracket.curly.function.definition.special.constructor.cpp (11 atoms) - from cpp.json
7. punctuation.section.block.end.bracket.curly.function.definition.special.member.destructor (11 atoms) - from cpp.json
8. punctuation.section.block.end.bracket.curly.function.definition.special.operator-overload.cpp (11 atoms) - from cpp.json
9. punctuation.section.arguments.begin.bracket.round.function.call.initializer.cpp (10 atoms) - from cpp.json
10. punctuation.section.arguments.begin.bracket.round.operator.sizeof.variadic.cpp (10 atoms) - from cpp.json
11. punctuation.section.arguments.end.bracket.round.function.call.initializer.cpp (10 atoms) - from cpp.json
12. punctuation.section.arguments.end.bracket.round.operator.sizeof.variadic.cpp (10 atoms) - from cpp.json
13. punctuation.section.block.begin.bracket.curly.function.definition.special.constructor (10 atoms) - from cpp.json
14. punctuation.section.block.begin.bracket.curly.function.definition.special.member (10 atoms) - from cpp.json
15. punctuation.section.block.begin.bracket.curly.function.definition.special.operator-overload (10 atoms) - from cpp.json
16. punctuation.section.block.end.bracket.curly.function.definition.special.constructor (10 atoms) - from cpp.json
17. punctuation.section.block.end.bracket.curly.function.definition.special.member (10 atoms) - from cpp.json
18. punctuation.section.block.end.bracket.curly.function.definition.special.operator-overload (10 atoms) - from cpp.json
19. punctuation.section.parameters.begin.bracket.round.special.member.destructor.cpp (10 atoms) - from cpp.json
20. punctuation.section.parameters.end.bracket.round.special.member.destructor.cpp (10 atoms) - from cpp.json

=== COMPARATIVE ANALYSIS (With vs Without C++ Grammars) ===
Extracting scopes from grammars-themes/packages/tm-grammars/grammars...
Found 15394 raw grammar scopes
Found 0 raw theme scopes
Generated 24225 total scopes (including hierarchy)
Extracting scopes from grammars-themes/packages/tm-grammars/grammars (excluding ["c", "cpp", "cpp-macro", "objective-c", "objective-cpp"])...
Found 14206 raw grammar scopes
Found 0 raw theme scopes
Generated 22420 total scopes (including hierarchy)

Metric                    | All Grammars |   Excluding C++ |   Difference
======================================================================
Total scopes              |        24225 |           22420 |  +1805 (+7.5%)
Unique atoms              |         3500 |            3419 |    +81 (+2.3%)
Average atoms per scope   |         3.94 |            3.86 |        +0.08
Max atoms per scope       |           12 |               8 |           +4

--- Longest Scopes (Excluding All C++ Grammars) ---
1. constant.character.escape.caret.control.key.emacs.lisp (8 atoms) - from emacs-lisp.json
2. constant.character.escape.octal.codepoint.key.emacs.lisp (8 atoms) - from emacs-lisp.json
3. invalid.illegal.php-code-in-comment.blade.meta.embedded.block.php (8 atoms) - from blade.json
4. invalid.illegal.php-code-in-comment.blade.meta.embedded.line.php (8 atoms) - from blade.json
5. keyword.operator.word.mnemonic.avx.promoted.simd-integer.mov (8 atoms) - from asm.json
6. keyword.operator.word.mnemonic.avx.promoted.supplemental.arithmetic (8 atoms) - from asm.json
7. keyword.operator.word.mnemonic.avx.promoted.supplemental.blending (8 atoms) - from asm.json
8. keyword.operator.word.mnemonic.avx.promoted.supplemental.logical (8 atoms) - from asm.json
9. keyword.operator.word.mnemonic.avx.promoted.supplemental.mov (8 atoms) - from asm.json
10. keyword.operator.word.mnemonic.supplemental.amd.3dnow.comparison (8 atoms) - from asm.json

Analysis complete!

```

IMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.``