#!/usr/bin/python3

import csv
import statistics 

from prettytable import PrettyTable

cores = [2, 4, 8, 16, 32, 64, 72]

# Create table 1
print()
print("---Time to add and remove a task from runqueue (in us) ---")
print()

single = PrettyTable()
single.field_names = ["Cores", "Spill free", "With state spill"]

for core in cores:
    spill_free_list = list()
    spillfull_list = list()
    with open('results/spill_free_single_%s.csv' %core) as csv_file:
        csv_reader = csv.reader(csv_file, delimiter=',')
        for row in csv_reader:
            spill_free_list.append(int(row[1]) / 100000.0)
            # 100000 is the number of tasks

    with open('results/spillful_single_%s.csv' %core) as csv_file:
        csv_reader = csv.reader(csv_file, delimiter=',')
        for row in csv_reader:
            spillfull_list.append(int(row[1]) / 100000.0)

    single.add_row([core, str(round(statistics.median(spill_free_list),2)) + " +/- " + str(round(statistics.stdev(spill_free_list), 2)), str(round(statistics.median(spillfull_list),2)) + " +/- " + str(round(statistics.stdev(spillfull_list), 2))]) 

print(single)

# Create table 2
print()
print()
print("---Time to spawn a task (in us) ---")
print()

whole = PrettyTable()
whole.field_names = ["Cores", "Spill free", "With state spill"]

for core in cores:
    spill_free_list = list()
    spillfull_list = list()
    with open('results/spill_free_whole_%s.csv' %core) as csv_file:
        csv_reader = csv.reader(csv_file, delimiter=',')
        for row in csv_reader:
            spill_free_list.append(int(row[1]) / 100.0)
            # 100 is the number of tasks

    with open('results/spillful_whole_%s.csv' %core) as csv_file:
        csv_reader = csv.reader(csv_file, delimiter=',')
        for row in csv_reader:
            spillfull_list.append(int(row[1]) / 100.0)

    whole.add_row([core, str(round(statistics.median(spill_free_list),2)) + " +/- " + str(round(statistics.stdev(spill_free_list), 2)), str(round(statistics.median(spillfull_list),2)) + " +/- " + str(round(statistics.stdev(spillfull_list), 2))]) 

print(whole)
