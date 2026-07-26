# Dofus Interface Analyzer

Portage Rust enrichi de l'ancien `DofusFontDumper`.

- `F11` analyse progressivement les arbres UI Toolkit.
- `F12` ajoute les propriétés visuelles et de layout résolues.
- `F10` réécrit l'inventaire live des `FontAsset`.
- Les résolutions dynamiques `Font -> FontAsset` sont capturées
  automatiquement par hook.

Le fichier principal retrouve son emplacement historique exact :
`UserData/dofus-font-dumper.log`. Chaque pression sur `F11` ou `F12` y ajoute
les en-têtes `UIDocument`, un parcours DFS indenté et les lignes compactes par
`VisualElement`.

Les JSONL et CSV restent disponibles en complément dans
`NativeMods/DofusInterfaceAnalyzer/<session>/`.

L'ancienne API HTTP capable de charger une assembly .NET n'est pas reproduite :
le host natif fournit déjà sa boîte aux lettres MCP `native-control.json` pour
charger, décharger et recharger des DLL Rust sur le thread Unity.
