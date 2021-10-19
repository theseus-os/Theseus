#!/bin/bash

for objfile in "$@"
do
	if grep -q ymm <(objdump -C -d $objfile)
	then
		echo -e "\033[0;32m$objfile has AVX instructions.\033[0m"
	elif grep -q xmm <(objdump -C -d $objfile) 
	then
		echo -e "\033[0;31m$objfile has SSE instructions.\033[0m"
	else
		echo "$objfile does NOT have SIMD instructions."
	fi
done

