( objdump  -DC $1 ; echo -e "\n==================================================================\n" ; readelf -aW $1 ) | cat | less
