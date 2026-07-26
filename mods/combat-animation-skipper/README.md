# Dofus Combat Animation Skipper

Mod Rust natif qui termine les animations `Animator2D` non bouclées dès leur
première mise à jour, uniquement pendant un combat.

Le mod ne met pas les animateurs en pause. Il avance le temps transmis à
`Animator2D.Run(float)`, ce qui laisse le moteur afficher la dernière frame,
mettre à jour `reachedEndOfAnimation` et émettre `AnimationEnded`. Les
animations bouclées restent à leur vitesse normale.

Les points d’entrée `Run`, `StartAnimation` et le cycle de vie du service de
combat sont retrouvés par signatures structurelles. Aucun nom de méthode
obfusqué de la version courante n’est codé en dur.
