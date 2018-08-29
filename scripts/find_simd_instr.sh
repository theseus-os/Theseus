
for objfile in "$@"
do
	if grep -q xmm <(objdump -C -d $objfile)
	then
		echo -e "\033[0;31m$objfile has SIMD instructions.\033[0m"
	else
		echo "$objfile does NOT have SIMD instructions."
	fi
done

