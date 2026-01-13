#let foo = [Hello]
#let bar(x) = [Hi #x]
#let title = none

#if title != none [Has Title] else [No Title]

#foo

#bar(1)

#for x in (1,2) [#x ]
