# Dofus Interface Analyzer

Portage Rust enrichi de l'ancien `DofusFontDumper`.

- `F11` analyse progressivement les arbres UI Toolkit.
- `F12` ajoute les propriétés visuelles et de layout résolues.
- `F10` réécrit l'inventaire live des `FontAsset`.
- Les résolutions dynamiques `Font -> FontAsset` sont capturées
  automatiquement par hook.

Les fichiers restent locaux dans
`NativeMods/DofusInterfaceAnalyzer/<session>/`.

L'ancienne API HTTP capable de charger une assembly .NET n'est pas reproduite :
le host natif fournit déjà sa boîte aux lettres MCP `native-control.json` pour
charger, décharger et recharger des DLL Rust sur le thread Unity.
