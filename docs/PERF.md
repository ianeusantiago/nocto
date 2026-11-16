# Nocto — medições de desempenho

Máquina: Ryzen 7 + RTX 4050, Windows 11. Medido com `scripts/measure.ps1` (working set, memória privada e CPU média no período).

| Data | Cenário | Working set | Privada | CPU |
|---|---|---|---|---|
| 2026-11-15 | Janela vazia, renderizador por software | 24,0 MB | 3,2 MB | 0,00% |
| 2026-11-15 | Janela vazia, renderizador femtovg (OpenGL) | 122,8 MB | 112,5 MB | 0,00% |
| 2026-11-15 | App completo, pílula parada (20 s) | 22,9 MB | 3,6 MB | 0,00% |
| 2026-11-15 | Barra de comando aberta (15 s) | 23,1 MB | 3,7 MB | 0,00% |
| 2026-11-15 | Calendário aberto (15 s) | 24,4 MB | 5,0 MB | 0,00% |
| 2026-11-15 | v0.2: vidro do compositor + janela fixa, pílula parada (20 s) | 26,6 MB | 5,2 MB | 0,00% |
| 2026-11-15 | Páginas + capa do álbum, pílula parada com relógio (20 s) | 29,9 MB | 5,7 MB | 0,00% |
| 2026-11-15 | Página de música aberta, música pausada (15 s) | 30,8 MB | 6,4 MB | 0,00% |
| 2026-11-17 | v0.3, antes da correção: música tocando, ondinha a cada quadro (20 s) | 35 MB | — | 0,32–0,46% |
| 2026-11-17 | v0.3: ondinha a ~11 quadros/s, pílula visível com música (20 s) | 36 MB | — | 0,24% |
| 2026-11-17 | v0.3: pílula escondida (janela cobrindo o topo / jogo) (20 s) | 34 MB | — | 0,01% |
| 2026-11-17 | v0.3: escuta do "Nocto" ligada, sala em silêncio (20 s) | 127 MB | — | +0,14% |

Executável release: ~8,8 MB.

## Decisões

- **Vidro pelo compositor do Windows (v0.2).** Desfoque, cor, brilho, borda de luz e morph rodam no compositor, numa janela própria atrás da interface. Custo: ~4 MB a mais; CPU parada continua 0%. A ondinha da música redesenha enquanto toca.

- **Renderizador por software.** O OpenGL (femtovg) custava ~100 MB a mais por carregar o driver de vídeo e foi removido do build. Também não disputa a GPU com jogos.
- **Nada em loop.** O atalho dorme em `GetMessageW`; a música chega por evento do Windows; a troca de tema é uma thread dormindo até a próxima virada; a detecção de tela cheia é um hook de evento fora de processo. Com a ilha parada, o Slint não desenha nenhum quadro.
- **Modo jogo.** Quando o app da frente ocupa o monitor inteiro, a pílula é escondida (`ShowWindow(SW_HIDE)`): zero pixels sobre o jogo. O atalho continua funcionando.
- `opt-level = "s"`, LTO, `panic = "abort"`, `strip`.

CPU em % do PC inteiro (16 núcleos): 0,24% ≈ 4% de um núcleo.

- **Ondinha da música (v0.3).** Redesenhava a cada quadro (≈8% de um núcleo) e continuava mesmo com a pílula escondida. Agora anda em passos de 90 ms e para quando a pílula não aparece.

## Ainda não medido

- FPS em jogo com e sem o Nocto (três execuções). Em tela cheia a pílula some e nada anima (0,01% de CPU, medido acima), então o esperado é diferença dentro do ruído; falta confirmar num jogo de verdade.
- Notebook mais fraco e Windows 10.
