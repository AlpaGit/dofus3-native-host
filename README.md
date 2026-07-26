# Dofus 3 Native Host

Runtime de mods natifs x64 pour Dofus 3 sous Unity IL2CPP, sans MelonLoader.

Le projet fournit :

- un bootstrap `version.dll` chargé naturellement par Unity ;
- un host de mods natifs avec une ABI C versionnée ;
- un SDK Rust partagé pour développer des mods sans recopier la logique IL2CPP ;
- un resolver global de classes, champs et méthodes par signatures structurelles ;
- un gestionnaire natif `F1` pour activer, désactiver, installer et mettre à jour les mods ;
- une marketplace pilotée par un simple `marketplace.json` hébergé sur GitHub ;
- un canal de contrôle permettant de charger, décharger et recharger une DLL ;
- le mod **Dofus 3 Native Tactical**, activable avec `F8`.

> [!IMPORTANT]
> Ce projet n’est ni développé, ni approuvé, ni distribué par Ankama. Une mise
> à jour de Dofus peut casser la compatibilité ou remplacer le `version.dll`
> installé. Utilisez-le à vos risques et périls.

## Mods

Le host ouvre son gestionnaire avec `F1`. La colonne de gauche affiche les DLL
déjà installées et permet de les **activer**, **désactiver** ou **recharger**.
La colonne de droite lit la marketplace GitHub et permet d’**installer** ou de
**mettre à jour** un mod. Une désactivation est persistante : le mod reste
présent sur disque, mais le host ne le charge plus aux lancements suivants.

### Dofus 3 Native Tactical

- Inclus dans l’installateur et le ZIP.
- `F8` active ou désactive le mode tactique sur la carte courante.
- Utilise le resolver structurel pour survivre aux changements de noms IL2CPP.
- Charge l’Addressable natif `tacticalCell` et réutilise directement ses
  sprites de déplacement et d’obstacle de ligne de vue.
- Instancie les 560 cellules par groupes de 28 afin de répartir le travail sur
  plusieurs ticks Unity.
- Code : [mods/native-tactical](https://github.com/AlpaGit/dofus3-native-host/tree/main/mods/native-tactical).

### Dofus Network Dumper

- Installable volontairement depuis la marketplace `F1` ; il n’est pas activé
  silencieusement par l’installateur.
- Capture les messages Protobuf entrants et sortants.
- Retrouve les handlers, getters et méthodes d’envoi par signatures, sans coder
  les noms obfusqués actuels en dur.
- Enregistre pour chaque paquet la direction, l’identifiant entrant, le vrai
  type IL2CPP concret, le payload wire décodé et les octets Protobuf en base64.
- Détecte structurellement les entrées de monstres des Songes Infinis dans le
  wire Protobuf, sans dépendre des noms obfusqués `iyn/fsuj`, puis exporte leur
  niveau et leurs statistiques.
- Sorties : `NativeMods/DofusNetworkDump/packets-*.jsonl` et
  `NativeMods/DofusNetworkDump/classes-*.json`, plus
  `infinite-dream-monsters-*.jsonl/.csv` lorsqu’un paquet compatible passe.
- Code : [mods/network-dumper](https://github.com/AlpaGit/dofus3-native-host/tree/main/mods/network-dumper).

> [!WARNING]
> Un dump réseau peut contenir des messages privés ou des données de compte.
> Ne publiez jamais le dossier `DofusNetworkDump` sans l’avoir relu et nettoyé.

### Dofus Runtime Inspector

- Installable volontairement depuis la marketplace `F1`.
- `F9` lance un snapshot progressif des renderers Unity afin de ne pas bloquer
  toute l’inspection dans une seule frame.
- Exporte les chemins de scène, transforms, bounds, sorting layers, matériaux,
  shaders, propriétés couleur/flottant/vecteur, sprites et meshes.
- Produit un `moquettes.csv` spécialisé avec un aperçu borné des vertices, UV
  et couleurs, ainsi que les textures référencées en PNG quand Unity permet
  leur copie.
- Ajoute un `manifest.json` récapitulatif et plafonne les exports lourds pour
  éviter une consommation mémoire ou disque incontrôlée.
- Sorties : `NativeMods/DofusRuntimeDump/<session>/`.
- Code : [mods/runtime-inspector](https://github.com/AlpaGit/dofus3-native-host/tree/main/mods/runtime-inspector).

### Dofus Interface Analyzer

- Remplace et élargit l’ancien mod MelonLoader `DofusFontDumper`.
- `F11` exporte progressivement les arbres UI Toolkit et `F12` ajoute les
  styles/layouts résolus, sans monopoliser une frame Unity.
- `F10` inventorie tous les `FontAsset`, leur famille, style, police source et
  textures d’atlas.
- Capture automatiquement les résolutions dynamiques
  `UnityEngine.Font -> TextCore FontAsset`.
- Produit des JSON/JSONL structurés et un CSV dédié aux textes, chemins UI et
  polices réellement utilisées.
- Requiert l’ABI v7 pour le test d’héritage IL2CPP, les chaînes UTF-8 sûres et
  le dispatch des méthodes virtuelles.
- Sorties : `NativeMods/DofusInterfaceAnalyzer/<session>/`.
- Code : [mods/interface-analyzer](https://github.com/AlpaGit/dofus3-native-host/tree/main/mods/interface-analyzer).

### Native Example

- Petit mod inclus pour vérifier l’ABI et servir de point de départ.
- Code : [mods/example](https://github.com/AlpaGit/dofus3-native-host/tree/main/mods/example).

## Installation rapide

### 1. Télécharger l’installateur

Téléchargez `Dofus3NativeHost-Setup-windows-x64.exe` depuis la
[dernière release](https://github.com/AlpaGit/dofus3-native-host/releases/latest).

### 2. Fermer Dofus

Fermez toutes les fenêtres Dofus avant de remplacer les DLL. Le launcher Ankama
peut rester ouvert.

### 3. Choisir le jeu

Lancez l’installateur. Il propose le chemin Ankama habituel et permet d’en
choisir un autre. Sélectionnez le dossier qui contient `Dofus.exe` ;
l’installateur le vérifie avant de continuer.

L’installateur communautaire n’est pas encore signé avec un certificat de
signature de code. Windows peut donc afficher SmartScreen ; vérifiez le
SHA-256 publié dans la release avant de l’exécuter.

L’emplacement habituel est :

```text
C:\Users\<vous>\AppData\Local\Ankama\Dofus-dofus3
```

Après installation, le dossier doit ressembler à ceci :

```text
Dofus-dofus3/
  Dofus.exe
  version.dll
  DofusNativeBootstrap.dll
  DofusNativeHost.dll
  NativeMods/
    DofusNativeExample.dll
    DofusNativeTactical.dll
```

### 4. Lancer

Lancez Dofus normalement depuis l’Ankama Launcher.

Appuyez sur `F1` pour ouvrir le gestionnaire de mods. Les changements
Activer/Désactiver sont conservés dans `NativeMods/native-mods.json` et restent
donc appliqués au prochain lancement.

Sur une carte, appuyez sur `F8` pour activer ou désactiver le mode tactique
natif. Le mod charge le prefab `tacticalCell` du jeu et utilise ses sprites
originaux au lieu de redessiner les cellules avec un mesh.

### Installation portable

La release contient aussi `Dofus3NativeHost-windows-x64.zip`. Pour une
installation manuelle, extrayez son contenu directement à côté de `Dofus.exe`.

## Mettre à jour

1. Fermez Dofus.
2. Téléchargez et relancez le nouvel installateur.
3. Sélectionnez le même dossier de jeu ; les fichiers sont remplacés.

Après certaines mises à jour du jeu, Ankama peut supprimer ou remplacer le
`version.dll` local. Il suffit alors de réinstaller la dernière release.

## Désinstaller

Utilisez **Applications installées > Dofus 3 Native Host > Désinstaller**.
L’installateur retire uniquement ses propres DLL et conserve les autres mods.

Pour une désinstallation manuelle, fermez Dofus puis supprimez uniquement :

```text
version.dll
DofusNativeBootstrap.dll
DofusNativeHost.dll
NativeMods/DofusNativeExample.dll
NativeMods/DofusNativeTactical.dll
NativeMods/native-control.json
NativeMods/native-mods.json
```

Les journaux et dossiers `NativeMods/control` peuvent également être supprimés.
Ne supprimez pas tout le dossier `NativeMods` si vous y avez ajouté vos propres
mods.

## Diagnostic

Les journaux se trouvent dans :

```text
NativeMods/native-bootstrap.log
NativeMods/native-host.log
```

Un démarrage sain contient notamment :

```text
Dofus Native Host ABI v7 starting
Dofus 3 Native Tactical negotiated native ABI v6
native mod manager ready; press F1 to open
Ready. Press F8 to toggle native tactical mode (Rust SDK, native sprites, ABI v6).
```

Si `F1` ou `F8` ne fait rien :

1. vérifiez que les cinq DLL ci-dessus sont au bon endroit ;
2. vérifiez `NativeMods/native-host.log` ;
3. réinstallez après toute mise à jour récente de Dofus ;
4. ouvrez une issue avec le journal, la version du jeu et les étapes exactes.

## Marketplace GitHub

Le gestionnaire télécharge le catalogue public :

```text
https://raw.githubusercontent.com/AlpaGit/dofus3-native-host/main/marketplace.json
```

Chaque entrée fournit l’identifiant ABI, le nom de fichier, l’URL HTTPS d’une
DLL publiée dans une GitHub Release et son SHA-256. Le host télécharge dans un
fichier temporaire, vérifie le hash avant tout chargement, sauvegarde l’ancienne
DLL lors d’une mise à jour et la restaure si la nouvelle DLL ne respecte pas
l’ABI.

Pour publier ou mettre à jour un mod :

1. publiez la DLL dans une GitHub Release ;
2. calculez son SHA-256 ;
3. modifiez l’entrée correspondante dans `marketplace.json` ;
4. vérifiez que `fileName` contient uniquement un nom de DLL, sans chemin.

Le schéma courant est `schemaVersion: 1`. Le workflow de release joint
automatiquement `DofusNativeTactical.dll`, `DofusNetworkDumper.dll`,
`DofusRuntimeInspector.dll`, `DofusInterfaceAnalyzer.dll` et leurs fichiers
`.sha256` aux nouveaux tags.

## Pourquoi une ABI v7 ?

Dofus 3 utilise Unity 6 et IL2CPP. Les noms du code du jeu peuvent être
obfusqués et changer entre deux builds. L’ABI évite donc de dépendre uniquement
de noms fragiles :

- résolution de classes, champs et méthodes par type et signature ;
- inspection des classes génériques construites ;
- invocation IL2CPP et remontée des exceptions ;
- création d’objets, chaînes et tableaux ;
- lecture et écriture de champs ;
- `GCHandle` natifs de taille pointeur.

Unity 6 utilise ici des handles 64 bits. L’ABI v4 les conserve sans troncature.
L’ABI v5 ajoute un service global de detours natifs avec trampoline, désactivation
et nettoyage automatique au déchargement d’un mod. C’est ce qui remplace
Harmony pour le dumper réseau. L’ABI v6 ajoute l’inflation globale des méthodes
IL2CPP génériques : un mod peut par exemple résoudre
`LoadAssetAsync<GameObject>` sans MelonLoader ni nom obfusqué. L’ABI v7 ajoute
le test d’assignabilité des classes, une conversion UTF-8 sûre des chaînes
IL2CPP et l’invocation virtuelle explicite, indispensables à l’analyse
générique de l’UI Toolkit. Les anciennes tables v1 à v6 restent prises en
charge.

## Développer un mod en Rust

Le workspace est séparé en trois couches :

```text
crates/mod-api   contrat FFI brut, stable et sans dépendance
crates/mod-sdk   API Rust commune et réutilisable
crates/host      chargeur, contrôle et pont UnityResolve
mods/*           DLL de mods
```

Un mod Rust est une bibliothèque `cdylib` qui exporte :

- `DNM_Query` pour négocier l’ABI et annoncer ses métadonnées ;
- `DNM_Load` pour initialiser le mod ;
- `DNM_Tick` appelé chaque frame sur le thread Unity ;
- `DNM_Unload` pour restaurer et libérer toutes les ressources.

Le SDK fournit notamment :

```rust
let runtime = unsafe { Runtime::bind(host_api) }.ok_or(DNH_ERROR)?;

let map_renderer = runtime.class(
    c"Core.dll",
    c"Core.Rendering",
    c"MapRenderer",
);

let map_id_fields =
    runtime.fields_by_type(map_renderer, c"System.Int64");

let method = runtime.instance_method(
    some_class,
    c"set_enabled",
    &[c"System.Boolean"],
);
```

`mods/native-tactical` est l’exemple complet recommandé. Sa logique générique
de signatures, d’invocation, d’unboxing, de tableaux, de logs et de handles se
trouve dans `crates/mod-sdk`, afin que les prochains mods ne la dupliquent pas.

## Compiler

Prérequis :

- Windows x64 ;
- Rust stable avec la cible `x86_64-pc-windows-msvc` ;
- Visual Studio 2022 Build Tools avec **Desktop development with C++** ;
- PowerShell.

```powershell
git clone https://github.com/AlpaGit/dofus3-native-host.git
cd dofus3-native-host
.\build.ps1 -Configuration Release
```

Le paquet installable est généré dans :

```text
dist/DofusRuntime/
```

Contrôles de qualité :

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Contrôle externe et MCP

À chaque démarrage, le host publie :

```text
NativeMods/native-control.json
```

Le descripteur pointe vers une boîte aux lettres locale traitée sur le thread
Unity. Les commandes disponibles sont :

- `STATUS` ;
- `LIST` ;
- `LOAD <chemin absolu de DLL>` ;
- `UNLOAD <id du mod>` ;
- `RELOAD <id du mod> <chemin absolu de DLL>`.

Cette surface permet à un outil MCP ou à un environnement de développement de
piloter les DLL sans injecteur supplémentaire. Le protocole reste local à la
session Dofus et les chemins sont validés par le host.

## Sécurité

Une DLL de mod s’exécute dans le processus Dofus et possède les mêmes droits que
le jeu. N’installez que des mods dont vous connaissez la provenance. Le host
valide le contrat ABI, mais ne constitue pas une sandbox.

Les captures de `Dofus Network Dumper` peuvent contenir des informations
sensibles. Elles restent locales et ne sont jamais envoyées par le host ou la
marketplace.

## Licence

Le code du projet est distribué sous licence MIT. UnityResolve est inclus sous
sa propre licence MIT. MinHook et son wrapper Rust conservent leurs licences
MIT/BSD. Les textes correspondants sont conservés dans `third_party/`.
