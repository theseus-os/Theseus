import re 
import string
from collections import defaultdict
from operator import itemgetter

"""
To use this script: 
First perform an objdump on kernel.bin in build/grub-isofiles/boot and copy the file into this folder as "objdump_output", then 
use the print_samples() function in the pmu_x86 crate to print the sampled instruction pointers and task IDs, and copy into this folder 
in a text file named "ips_sampled". 
"""

instruction_regex = re.compile('(\\s{2,})([a-z])+(?=\\s+)') # Regex to capture instructions types 
function_regex = re.compile('<(.)*>:') # Regex to capture function names from objdump
bin_file = open("objdump_output", )
ip_file = open("ips_sampled", )
ip_list = [] # List of instruction pointers retrieved from ip_file
id_list = [] # Task ID index matches with IP it came from
function_instruction = [] # Matches each function and instruction recorded in a list of tuples
ip_frequency_per_id = defaultdict(lambda: defaultdict(int))
function_frequency_per_id = defaultdict(lambda: defaultdict(int))
ip_frequency = defaultdict(int)
function_freq = defaultdict(int)
enclosing_function = ""

# Iterates through files that store IPs and associated task ID and places them into lists
for line in ip_file:
    ip_list.append(line[4:20])
    id_list.append(line[21:22])


ip_list_len = len(ip_list)*1.0

# Iterates through the objdump output file. For each line in the file, it checks to see if the IP is contained in it and if it is, 
# checks to see if the line has a decipherable instruction in it and records the instruction and enclosing function if it does.
# Also records the function that the IP is enclosed in by keeping track of the most recent function name in the file. 
for line in bin_file:
    possible_function = function_regex.search(line)
    if not possible_function == None:
        enclosing_function = possible_function.group(0)
    for idx, ip in enumerate(ip_list):
        if ip in line:
            m = instruction_regex.search(line)
            if not m == None:
                ip_frequency_per_id[id_list[idx]][m.group(0).lstrip()] += 1
                function_frequency_per_id[id_list[idx]][enclosing_function] += 1
                function_instruction.append((enclosing_function, m.group(0).lstrip()))
            else: 
                ip_frequency_per_id[id_list[idx]]["Function Header IP (no associated instruction)"] += 1
                function_frequency_per_id[id_list[idx]][enclosing_function] += 1
                function_instruction.append((enclosing_function, "Function Header IP"))

            ip_list.remove(ip)

# Prints the frequencies of the instructions and tasks. 
for id in function_frequency_per_id.keys():
    print "FOR ID: {}".format(id)
    for function, freq in sorted(function_frequency_per_id[id].items(), key=itemgetter(1), reverse=True):
        print  function, freq/ip_list_len
    for instruction, freq in sorted(ip_frequency_per_id[id].items(), key=itemgetter(1), reverse=True):
        print instruction, freq/ip_list_len

print "Samples taken: {}".format(ip_list_len)
