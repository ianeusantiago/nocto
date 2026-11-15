# Nocto — medições de desempenho

Máquina: Ryzen 7 + RTX 4050, Windows 11. Medido com `scripts/measure.ps1` (working set, memória privada e CPU média no período).

| Data | Cenário | Working set | Privada | CPU |
|---|---|---|---|---|
| 2026-11-15 | Janela vazia, renderizador por software | 24,0 MB | 3,2 MB | 0,00% |
| 2026-11-15 | Janela vazia, renderizador femtovg (OpenGL) | 122,8 MB | 112,5 MB | 0,00% |
| 2026-11-15 | App completo, pílula parada (20 s) | 22,9 MB | 3,6 MB | 0,00% |
| 2026-11-15 | Barra de comando aberta (15 s) | 23,1 MB | 3,7 MB | 0,00% |
| 2026-11-15 | Calendário aberto (15 s) | 24,4 MB | 5,0 MB | 0,00% |

Executável release: ~8,8 MB.

## Decisões

- **Renderizador por software.** O OpenGL (femtovg) custava ~100 MB a mais por carregar o driver de vídeo e foi removido do build. Também não disputa a GPU com jogos.
- **Nada em loop.** O atalho dorme em `GetMessageW`; a música chega por evento do Windows; a troca de tema é uma thread dormindo até a próxima virada; a detecção de tela cheia é um hook de evento fora de processo. Com a ilha parada, o Slint não desenha nenhum quadro.
- **Modo jogo.** Quando o app da frente ocupa o monitor inteiro, a pílula é escondida (`ShowWindow(SW_HIDE)`): zero pixels sobre o jogo. O atalho continua funcionando.
- `opt-level = "s"`, LTO, `panic = "abort"`, `strip`.

## Ainda não medido

- FPS em jogo com e sem o Nocto (três execuções), pedido na Task 10.
- Notebook mais fraco e Windows 10.
