# Question set

`questions.jsonl` is the fixed set we use to check that a small local model
answers more accurately with the library than without it. Each line has a
question and strings, any one of which, found in the answer, counts as right.
The first 24 are general knowledge; the last 16 are long-tail facts taken
from Wikipedia articles, which a small model mostly doesn't know.

`run.py` asks each question twice through an OpenAI-compatible server
(llamafile / llama-server): once alone, and once with the tools of
`zim-mcp serve`, spoken to over MCP exactly as any client would, with the
server's instructions as the system prompt.

```sh
llamafile.exe --server -m model.gguf --port 8080 -ngl 999 -c 16384
python3 eval/run.py --zim /path/to/zim --zim-mcp zim-mcp
```

Results go to `eval/results.json`. Keep the questions fixed so results stay
comparable across models and releases.

Last run (2026-11-15): Qwen3-4B-Instruct-2507 Q4_K_M, `wikipedia_en_top1m_mini_2026-04`:

| | without library | with library |
|---|---|---|
| general (24) | 24 | 24 |
| long-tail (16) | 3 | 11 |
