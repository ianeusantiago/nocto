# Nocto

Uma "Dynamic Island" para Windows, escrita em Rust. Uma ilha no estilo Dynamic Island, colada no topo da tela, com material de vidro inspirado no Liquid Glass, que vira barra de comando, calendário e conversor de imagens, e fica leve o bastante para não ser notada nem em jogo.

**~30 MB de RAM e 0% de CPU parada** ([medições](docs/PERF.md)). Executável único de ~9 MB, sem navegador embutido.

## O que faz

| | |
|---|---|
| **Atalho global** `Alt + Shift + Espaço` | Abre a barra de comando de qualquer app, inclusive jogos. `Esc` fecha e devolve o foco. |
| **Comandos** | `abre spotify` · `abre github.com` · `abre C:\Projetos` · `toca <música>` · `pausa` · `próxima` · `agenda` · `conectar spotify` · `sair` |
| **Música** | Clique na ilha: capa do álbum, nome, artista, tempo e controles (⏮ ⏯ ⏭). Logo do Spotify quando a música vem dele. Funciona com qualquer player do Windows, sem login. |
| **Páginas** | A ilha expandida tem páginas (música e calendário). Troque arrastando, com a roda do mouse, com as setas ← → ou clicando nas bolinhas do topo. |
| **Configurações** | Terceira página da ilha: tema, o que a pílula mostra, esconder em jogos e atalho. **Arraste um app ou atalho** (ex.: o Brave da área de trabalho) até a ilha e `abre brave` passa a funcionar; dá para remover na mesma lista. |
| **Volume** | Knob na página de música (arraste ou gire a roda do mouse) que controla o volume do Spotify no mixer do Windows. |
| **Playlist** | `toca playlist only day` toca uma playlist sua pelo nome (digite `conectar spotify` uma vez antes). |
| **Pílula** | Mostra a música ou o relógio. O botão "Pílula" na página de música alterna entre automático (música só tocando), música e relógio. |
| **Calendário** | Segunda página da ilha. Escolha o dia e digite `14:00 reunião` + Enter. Clique no item para concluir. Tudo fica num arquivo local. |
| **Converter imagem** | Arraste uma imagem até a pílula e escolha PNG, JPEG, WebP ou **Remover fundo** (local e grátis; baixa um modelo de 44 MB no primeiro uso, leva uns 10 s). O arquivo novo é salvo ao lado do original, sem sobrescrever. |
| **Tema por horário** | Azul-céu das 05h às 16h, laranja até as 18h, azul-noite depois. Troca com fade. O botão no calendário fixa um tema. |
| **Modo jogo** | Se o app da frente está em tela cheia, a pílula some sozinha. O atalho continua valendo. |
| **Mensagens** | WhatsApp e e-mail (Outlook, ou Gmail avisando pelo navegador) chegam pelas notificações do Windows: a ilha expande por 5 s e a página Mensagens guarda as últimas, com abas WhatsApp | E-mail. Nas Configurações dá para desligar ou esconder o texto. |
| **Voz** | Botão de microfone na barra: clique e fale o comando ("abre brave", "pausa", "agenda"). 100% offline e grátis (Vosk, modelo em português de ~50 MB baixado no primeiro uso). Nomes em inglês às vezes saem errados. |
| **"Nocto"** | Opcional (Configurações › Chamar dizendo "Nocto"): dizer o nome abre a barra e já ouve o comando. Microfone fica ligado; custa ~+90 MB de RAM e quase nada de CPU em silêncio; pausa em jogo de tela cheia. |

## Configuração

Fica em `%APPDATA%\Nocto\`:

- `config.json`: `hotkey` (ex.: `"ctrl+alt+n"`), `theme` (`auto`, `dia`, `por-do-sol`, `noite`), `spotify_client_id`, `hide_in_fullscreen`.
- `allowlist.json`: os **únicos** apps que `abre <nome>` pode abrir (nome → executável ou URI). Já vem com Spotify, Bloco de Notas, Calculadora, Terminal e outros, mais Chrome/VS Code/Discord/Steam se estiverem instalados.
- `calendar.json`: seus compromissos. Se o arquivo corromper, o Nocto abre vazio e guarda uma cópia em `calendar.json.bak`.

Reinicie o Nocto (`sair` na barra e abra de novo) depois de editar.

### "toca X" no Spotify (opcional, precisa de Premium)

1. Crie um app grátis em <https://developer.spotify.com/dashboard>.
2. Em **Redirect URIs**, adicione `http://127.0.0.1:8898/callback` e marque **Web API**.
3. Copie o **Client ID** para `spotify_client_id` no `config.json`.
4. Na barra, digite `conectar spotify` e autorize no navegador.

No modo de desenvolvimento, o Spotify limita quem pode usar o app aos usuários que você cadastrar no painel. Sem isso, play/pause/próxima continuam funcionando pelos controles do Windows.

## Compilar

```bash
cargo build --release
```

```bash
cargo test -p nocto-core
```

A lógica (comandos, calendário, tema, conversão, Spotify, configuração) fica em `core/` e é testada sem interface. A janela e a integração com o Windows ficam em `src/`, e o visual em `ui/island.slint` (Slint).

## Limitações conhecidas

- Jogos em **tela cheia exclusiva** não mostram janelas sobrepostas. Em janela sem borda funciona.
- O desfoque usa uma API não documentada do Windows (`SetWindowCompositionAttribute`), que funciona no Windows 10 1803+ e no 11.
- Se um jogo entra em tela cheia sem trocar de janela (Alt+Enter), a pílula só some na próxima troca de foco.
- O vidro precisa dos "Efeitos de transparência" ligados no Windows; sem eles, a ilha fica com cor sólida.
