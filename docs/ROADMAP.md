# Nocto — próximos passos

Tudo gratuito, sem serviço pago. Ordem definida pelo autor: **A, D, E, B, C**, depois o resto, a partir do F.

| # | Item | Como (resumo) | Observações |
|---|---|---|---|
| A | ✅ **Release nova no GitHub** | Gerar e publicar a 0.2.0 com o vidro, as páginas e as correções. | Rápido. |
| D | ✅ **Tocar uma playlist** | `toca playlist only day` → procura nas suas playlists e toca. | Precisa de uma permissão a mais do Spotify (`playlist-read-private`): um novo "conectar spotify" uma vez. |
| E | ✅ **Volume com knob** | Página de música ganha um knob giratório (estilo FL Studio, no visual do Nocto) ou barra, trocável depois. | Volume do próprio Spotify no mixer do Windows: grátis, local e funciona com qualquer player. |
| B | ✅ **Configurações** (aba na ilha) | Página nova: tema, o que a pílula mostra, atalho, esconder em tela cheia. | Base para C. |
| C | ✅ **Adicionar apps por arraste** | Em Configurações → Comandos → "Adicionar app": a ilha pede para arrastar um app (.exe ou atalho .lnk); o Nocto lê o nome e o ícone, mostra uma animação de "adicionado" e daí `abre brave` funciona. Lista com remover. | Grava na `allowlist.json` (que hoje só se edita à mão). |
| F ✅ | **Notificações do WhatsApp** | Página/aviso na ilha quando chega mensagem. | Lê as notificações do app WhatsApp do Windows (API de notificações do Windows); grátis. Pede permissão de acesso às notificações uma vez. |
| G ✅ | **Remover fundo de imagem** | Botão a mais ao soltar uma imagem. | Modelo de IA baixado só no primeiro uso. |
| H ✅ | **E-mail** | Página com e-mails não lidos do Gmail. | Login com o Google uma vez. |
| I ✅ | **Comando por voz** | Falar com a barra em vez de digitar. | Reconhecimento de fala local (grátis, mais pesado). |
| J | **Medir FPS em jogo** | Com e sem o Nocto, três execuções. | Prioridade de leveza ainda sem número. |
| K | **Testar em PC fraco / Windows 10** | Notebook do amigo. | |
| L | **Testar arrastar arquivo de verdade** | Converter imagem soltando na ilha. | Nunca testado com arraste real. |
| M ✅ | **Logo do Nocto** | Ícone do app, do executável e do README. | Uma das últimas. |
| N | **Área de transferência** | Página com o que foi copiado (texto, link, código colorido, imagem, cor, arquivo) e preview que adapta o tamanho da ilha. | Em andamento. |
| O | **Modo karaokê** | Botão de microfone na página de música abre a letra sincronizada passando na tela, de um jeito divertido. | Próximo, depois da N. |
| P | **Tocar álbum** | `toca álbum Late Registration by Kanye West` toca o álbum inteiro. | Depois do karaokê. |
| Q | **Fila do Spotify** | `coloca na fila <música>` põe a música na fila; a próxima que tocar é ela. | Junto com álbuns. |
